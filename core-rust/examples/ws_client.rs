use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::io::{self, AsyncBufReadExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

#[tokio::main]
async fn main() {
  let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| "ws://localhost:2953/".to_string());
  let auth_token = std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| "test-token".to_string());
  let user_name = std::env::var("USER_NAME").unwrap_or_else(|_| "test-user".to_string());

  println!("Connecting to: {}", ws_url);
  println!("Auth: {}:{}", user_name, auth_token);

  let url = match Url::parse(&ws_url) {
    Ok(value) => value,
    Err(err) => {
      eprintln!("Invalid WS_URL: {}", err);
      return;
    }
  };

  let (ws_stream, _) = match connect_async(url).await {
    Ok(result) => result,
    Err(err) => {
      eprintln!("WebSocket connect error: {}", err);
      return;
    }
  };

  println!("✅ Connected to WebSocket server");
  println!("📝 Type messages and press Enter (Ctrl+D to exit)");

  let (mut ws_sender, mut ws_receiver) = ws_stream.split();
  if let Err(err) = ws_sender
    .send(Message::Text(format!("{}:{}", user_name, auth_token)))
    .await
  {
    eprintln!("Auth send failed: {}", err);
    return;
  }

  let stdin = io::BufReader::new(io::stdin());
  let mut lines = stdin.lines();

  let read_task = tokio::spawn(async move {
    while let Some(message) = ws_receiver.next().await {
      match message {
        Ok(Message::Text(text)) => {
          if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
            println!("\n📨 Received:\n{}", serde_json::to_string_pretty(&parsed).unwrap());
          } else {
            println!("\n📨 Raw message: {}", text);
          }
        }
        Ok(Message::Close(_)) => {
          println!("\n❌ Connection closed");
          break;
        }
        Ok(_) => {}
        Err(err) => {
          eprintln!("\n💥 WebSocket error: {}", err);
          break;
        }
      }
    }
  });

  while let Ok(Some(line)) = lines.next_line().await {
    let trimmed = line.trim();
    if trimmed.is_empty() {
      continue;
    }

    let (kind, text) = if let Some(rest) = trimmed.strip_prefix("sensory:") {
      ("sensory", rest.trim())
    } else {
      ("message", trimmed)
    };

    let payload = json!({
      "type": kind,
      "text": text,
    });

    println!("📤 Sending: {}", payload);
    if let Err(err) = ws_sender
      .send(Message::Text(payload.to_string()))
      .await
    {
      eprintln!("Send error: {}", err);
      break;
    }
  }

  let _ = ws_sender.send(Message::Close(None)).await;
  let _ = read_task.await;
}
