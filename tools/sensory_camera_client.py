#!/usr/bin/env python3
"""
Capture a photo with fswebcam, ask OpenAI to describe it, and send the result
to Tsuki as a sensory WebSocket message. Uses only the standard library and
the fswebcam binary.
"""

import base64
import dataclasses
import hashlib
import json
import os
import socket
import ssl
import struct
import subprocess
import sys
import shutil
import tempfile
import time
from typing import Dict, Optional, Tuple
from urllib.parse import urlparse
from urllib.request import Request, urlopen

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


@dataclasses.dataclass
class Config:
    ws_url: str
    user: str
    token: str
    openai_api_key: str
    openai_model: str
    capture_cmd: Tuple[str, ...]
    openai_base_url: str
    request_timeout: int
    reply_window: int


def load_config() -> Config:
    ws_url = os.getenv("WS_URL", "ws://localhost:2953/")
    user = os.getenv("USER_NAME", "camera-user")
    token = os.getenv("WEB_AUTH_TOKEN", "test-token")
    openai_api_key = os.getenv("OPENAI_API_KEY")
    if not openai_api_key:
        sys.exit("OPENAI_API_KEY is required")

    openai_model = os.getenv("OPENAI_MODEL", "gpt-4o-mini")
    openai_base_url = os.getenv("OPENAI_BASE_URL", "https://api.openai.com")
    request_timeout = int(os.getenv("OPENAI_TIMEOUT_SECONDS", "30"))
    reply_window = int(os.getenv("WS_REPLY_WINDOW_SECONDS", "10"))

    capture_cmd = ("fswebcam", "--device", "/dev/video0", "--input", "0", "--resolution", "1920x1080", "--no-banner")

    return Config(
        ws_url=ws_url,
        user=user,
        token=token,
        openai_api_key=openai_api_key,
        openai_model=openai_model,
        capture_cmd=capture_cmd,
        openai_base_url=openai_base_url.rstrip("/"),
        request_timeout=request_timeout,
        reply_window=reply_window,
    )


def ensure_fswebcam_available(cmd: Tuple[str, ...]) -> None:
    if not cmd:
        sys.exit("FSWEBCAM_CMD must not be empty")
    binary = cmd[0]
    if not shutil.which(binary):
        sys.exit(f"{binary} not found. Install fswebcam or override with FSWEBCAM_CMD")


def capture_image(cmd: Tuple[str, ...]) -> bytes:
    ensure_fswebcam_available(cmd)
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as tmp_file:
        target_path = tmp_file.name
    full_cmd = list(cmd) + [target_path]
    result = subprocess.run(full_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        if result.returncode != 0:
            stderr = result.stderr.decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"fswebcam failed: {stderr or 'unknown error'}")
        with open(target_path, "rb") as img_file:
            return img_file.read()
    finally:
        try:
            os.remove(target_path)
        except OSError:
            pass


def describe_image(cfg: Config, image_bytes: bytes) -> str:
    image_b64 = base64.b64encode(image_bytes).decode("ascii")
    payload = {
        "model": cfg.openai_model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": """Describe the photo according to following format in Japanese 数単語:
- 人物の行動・使っているもの
- 人物の様子
- 部屋の明るさ・時間帯
""",
                    },
                    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{image_b64}" }},
                ],
            }
        ],
        "max_tokens": 150,
    }

    url = f"{cfg.openai_base_url}/v1/chat/completions"
    data = json.dumps(payload).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {cfg.openai_api_key}",
    }

    request = Request(url, data=data, headers=headers, method="POST")
    try:
        with urlopen(request, timeout=cfg.request_timeout) as response:
            content = response.read()
    except Exception as err:
        raise RuntimeError(f"OpenAI request failed: {err}") from err

    try:
        parsed = json.loads(content)
    except json.JSONDecodeError as err:
        raise RuntimeError(f"OpenAI response was not JSON: {err}") from err

    try:
        message = parsed["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError) as err:
        raise RuntimeError(f"OpenAI response missing content: {parsed}") from err

    return str(message).strip()


class SimpleWebSocketClient:
    def __init__(self, url: str) -> None:
        self.url = url
        self.sock: Optional[socket.socket] = None
        self.buffer = b""

    def connect(self) -> None:
        parsed = urlparse(self.url)
        if parsed.scheme not in ("ws", "wss"):
            raise ValueError(f"Unsupported scheme: {parsed.scheme}")

        host = parsed.hostname
        if not host:
            raise ValueError("WebSocket URL must include a host")

        port = parsed.port or (443 if parsed.scheme == "wss" else 80)
        path = parsed.path or "/"
        if parsed.query:
            path = f"{path}?{parsed.query}"

        key = base64.b64encode(os.urandom(16)).decode("ascii")
        headers = [
            f"GET {path} HTTP/1.1",
            f"Host: {host}:{port}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {key}",
            "Sec-WebSocket-Version: 13",
            "Origin: http://localhost",
        ]

        sock: socket.socket = socket.create_connection((host, port), timeout=10)
        if parsed.scheme == "wss":
            context = ssl.create_default_context()
            sock = context.wrap_socket(sock, server_hostname=host)

        request = "\r\n".join(headers) + "\r\n\r\n"
        sock.sendall(request.encode("ascii"))

        response, leftover = self._read_http_response(sock)
        self.buffer = leftover

        expected_accept = base64.b64encode(
            hashlib.sha1(f"{key}{GUID}".encode("ascii")).digest()
        ).decode("ascii")
        actual_accept = response["headers"].get("sec-websocket-accept")
        if response["status"] != 101 or actual_accept != expected_accept:
            raise RuntimeError(f"WebSocket handshake failed: status={response['status']} accept={actual_accept}")

        self.sock = sock

    def send_text(self, text: str) -> None:
        if not self.sock:
            raise RuntimeError("WebSocket is not connected")
        frame = self._encode_frame(text.encode("utf-8"), opcode=0x1)
        self.sock.sendall(frame)

    def send_json(self, payload: Dict) -> None:
        self.send_text(json.dumps(payload, ensure_ascii=False))

    def read_messages(self, window_seconds: int) -> None:
        if not self.sock:
            return
        end_time = time.time() + window_seconds
        while time.time() < end_time:
            remaining = max(0.1, end_time - time.time())
            try:
                message = self._recv_frame(timeout=remaining)
            except socket.timeout:
                continue
            if not message:
                continue

            opcode, payload = message
            if opcode == 0x1:
                print(f"📨 {payload.decode('utf-8', errors='replace')}")
            elif opcode == 0x9:  # ping
                self._send_pong(payload)
            elif opcode == 0x8:  # close
                break

    def close(self) -> None:
        if self.sock:
            try:
                self.sock.sendall(self._encode_frame(b"", opcode=0x8))
            except OSError:
                pass
            try:
                self.sock.close()
            finally:
                self.sock = None

    def _read_http_response(self, sock: socket.socket) -> Tuple[Dict, bytes]:
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk

        header_bytes, _, leftover = data.partition(b"\r\n\r\n")
        lines = header_bytes.split(b"\r\n")
        status_line = lines[0].decode("ascii", errors="replace")
        try:
            status = int(status_line.split()[1])
        except (IndexError, ValueError):
            status = -1

        headers: Dict[str, str] = {}
        for line in lines[1:]:
            if b":" not in line:
                continue
            name, value = line.split(b":", 1)
            headers[name.decode("ascii", errors="replace").lower()] = value.strip().decode("ascii", errors="replace")

        return {"status": status, "headers": headers}, leftover

    def _recv_frame(self, timeout: float) -> Optional[Tuple[int, bytes]]:
        if not self.sock:
            return None
        self.sock.settimeout(timeout)
        try:
            header = self._read_exact(2)
        except socket.timeout:
            return None
        b1, b2 = header
        opcode = b1 & 0x0F
        masked = b2 & 0x80
        length = b2 & 0x7F

        if length == 126:
            length_bytes = self._read_exact(2)
            length = struct.unpack("!H", length_bytes)[0]
        elif length == 127:
            length_bytes = self._read_exact(8)
            length = struct.unpack("!Q", length_bytes)[0]

        mask_key = self._read_exact(4) if masked else None
        payload = self._read_exact(length) if length else b""

        if masked and mask_key:
            payload = bytes(b ^ mask_key[i % 4] for i, b in enumerate(payload))

        return opcode, payload

    def _send_pong(self, payload: bytes) -> None:
        if not self.sock:
            return
        frame = self._encode_frame(payload, opcode=0xA)
        try:
            self.sock.sendall(frame)
        except OSError:
            pass

    def _read_exact(self, length: int) -> bytes:
        if length == 0:
            return b""
        while len(self.buffer) < length:
            chunk = self.sock.recv(length - len(self.buffer))
            if not chunk:
                raise ConnectionError("WebSocket connection closed")
            self.buffer += chunk
        data, self.buffer = self.buffer[:length], self.buffer[length:]
        return data

    def _encode_frame(self, payload: bytes, opcode: int) -> bytes:
        fin_opcode = 0x80 | opcode
        mask_bit = 0x80
        length = len(payload)

        if length < 126:
            header = struct.pack("!BB", fin_opcode, mask_bit | length)
        elif length < (1 << 16):
            header = struct.pack("!BBH", fin_opcode, mask_bit | 126, length)
        else:
            header = struct.pack("!BBQ", fin_opcode, mask_bit | 127, length)

        mask_key = os.urandom(4)
        masked_payload = bytes(b ^ mask_key[i % 4] for i, b in enumerate(payload))
        return header + mask_key + masked_payload


def main() -> None:
    dry_run = sys.argv.count("--dry-run") > 0

    cfg = load_config()
    try:
        image_bytes = capture_image(cfg.capture_cmd)
    except Exception as err:
        sys.exit(f"Image capture failed: {err}")

    print(f"📸 Captured image ({len(image_bytes)} bytes)")

    try:
        description = describe_image(cfg, image_bytes)
    except Exception as err:
        sys.exit(f"Failed to describe image: {err}")

    print(f"✅ Image described: {description}")

    if dry_run:
        print("🛑 Dry run mode, not sending to WebSocket")
        return

    client = SimpleWebSocketClient(cfg.ws_url)
    try:
        client.connect()
    except Exception as err:
        sys.exit(f"WebSocket connection failed: {err}")

    auth_message = f"{cfg.user}:{cfg.token}"
    try:
        client.send_text(auth_message)
        client.send_json({"type": "sensory", "text": description})
        print("📤 Sent sensory message, waiting for replies...")
        client.read_messages(cfg.reply_window)
    except Exception as err:
        sys.exit(f"WebSocket interaction failed: {err}")
    finally:
        client.close()


if __name__ == "__main__":
    main()
