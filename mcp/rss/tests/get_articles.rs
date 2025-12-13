use std::{env, fs, time::Duration};

use tempfile::TempDir;

use rss_mcp::{config::RssConfig, service::{GetArticlesRequest, RssService}};

fn write_config(dir: &TempDir, feeds: &[&str]) {
    let yaml = format!(
        "feeds:\n{}\n",
        feeds
            .iter()
            .map(|url| format!("  - {}", url))
            .collect::<Vec<_>>()
            .join("\n")
    );
    fs::write(dir.path().join("rss.yaml"), yaml).expect("write rss.yaml");
}

#[tokio::test]
async fn config_uses_default_timeout_when_env_missing() {
    let dir = TempDir::new().expect("temp dir");
    write_config(&dir, &["https://example.com/feed.xml"]);

    unsafe {
        env::set_var("DATA_DIR", dir.path());
        env::set_var("TZ", "UTC");
        env::remove_var("FEED_TIMEOUT_SECONDS");
    }

    let config = RssConfig::from_env().await.expect("config");
    assert_eq!(config.feed_timeout, Duration::from_secs(2));
}

#[tokio::test]
async fn config_honors_env_timeout_override() {
    let dir = TempDir::new().expect("temp dir");
    write_config(&dir, &["https://example.com/feed.xml"]);

    unsafe {
        env::set_var("DATA_DIR", dir.path());
        env::set_var("TZ", "UTC");
        env::set_var("FEED_TIMEOUT_SECONDS", "5");
    }

    let config = RssConfig::from_env().await.expect("config");
    assert_eq!(config.feed_timeout, Duration::from_secs(5));
}

#[tokio::test]
async fn get_articles_filters_since_and_limits() {
    let server = httpmock::MockServer::start();
    let feed_url = server.url("/feed");

    let feed_body = r#"
        <?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0">
          <channel>
            <title>Sample Feed</title>
            <link>https://example.com/</link>
            <description>Sample</description>
            <item>
              <title>Newer</title>
              <link>https://example.com/newer</link>
              <pubDate>Sat, 13 Dec 2025 10:00:00 GMT</pubDate>
              <description>Newer desc</description>
            </item>
            <item>
              <title>Older</title>
              <link>https://example.com/older</link>
              <pubDate>Fri, 12 Dec 2025 10:00:00 GMT</pubDate>
              <description>Older desc</description>
            </item>
          </channel>
        </rss>
    "#;

    let _mock = server.mock(|when, then| {
        when.path("/feed");
        then.status(200)
            .header("content-type", "application/rss+xml")
            .body(feed_body);
    });

    let dir = TempDir::new().expect("temp dir");
    write_config(&dir, &[feed_url.as_str()]);

    unsafe {
        env::set_var("DATA_DIR", dir.path());
        env::set_var("TZ", "Asia/Tokyo");
        env::remove_var("FEED_TIMEOUT_SECONDS");
    }

    let service = RssService::from_env().await.expect("service");

    let response = service
        .get_articles(rmcp::handler::server::wrapper::Parameters(
            GetArticlesRequest {
                since: Some("2025-12-13T00:00:00Z".to_string()),
                n: Some(1),
            },
        ))
        .await
        .expect("get_articles");

    let text = response
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("text content");
    assert!(
        text.contains("articles[1]{title,url,published_at,description}:"),
        "response should be TOON formatted"
    );
    assert!(
        text.contains("Newer,https://example.com/newer,2025-12-13T19:00:00+09:00,Newer desc"),
        "should include newer entry with TZ applied"
    );
    assert!(
        !text.contains("Older"),
        "older entry should be filtered by since + limit"
    );
}
