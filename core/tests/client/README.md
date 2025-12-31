# WebSocket test client

coreに接続してシナリオを流すための簡易クライアント。評価は人間が行い、クライアントは送受信ログを残すだけ。

## 目的
- coreのWebSocketに対して決め打ちの入力を順に送る
- 受信内容をJSONLで保存して、後から人間が確認できるようにする

## 前提
- プロトコル定義は `api-specs/asyncapi.yaml` を参照
- 認証は `USER_NAME:WEB_AUTH_TOKEN` を最初に送る方式（`core/scripts/ws_client.js` と同じ）

## 使い方（想定）
```
cd core
pnpm tsx ./tests/client/run.ts ./tests/client/scenarios/example.yaml
```

## 環境変数
- `WS_URL` (default: `ws://localhost:2953/`)
- `WEB_AUTH_TOKEN` (default: `test-token`)
- `USER_NAME` (default: `test-user`)

## ログ
- pinoのJSONL出力をファイルに保存
- 送信・受信・エラーを同じログに並べる
- 保存先の例: `core/tests/client/logs/20250101-120000.jsonl`

## シナリオ形式（YAML）
入力メッセージのリストだけを扱う。順に送信する。

```yaml
# example.yaml
inputs:
  - text: "こんにちは"
  - text: "今日の気分は？"
  - type: sensory
    text: "雨の匂いがする"
  - text: "長い処理が必要な入力"
    timeout_ms: 120000
```

- `type`の指定が無い場合は`type: message`として送る
- `timeout_ms` がある場合はその入力に対する待機時間として使う

## 期待する挙動
- `inputs` を上から順に送信
- 全ステップ送信後に終了
- 受信内容は評価せず、そのままログに残す

## ディレクトリ案
```
core/tests/client/
  README.md
  run.ts
  scenarios/
    example.yaml
  logs/
```
