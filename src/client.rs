use serde::Serialize;
use std::future::Future;
use std::pin::Pin;

use crate::{dispatch::LogEvent, export::LogDestination};

#[derive(Debug)]
pub enum BetterstackError {
    HttpError(reqwest::Error),
    InvalidConfig(String),
}

impl std::fmt::Display for BetterstackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BetterstackError::HttpError(e) => write!(f, "HTTP error: {}", e),
            BetterstackError::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
        }
    }
}

impl std::error::Error for BetterstackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BetterstackError::HttpError(e) => Some(e),
            BetterstackError::InvalidConfig(_) => None,
        }
    }
}

impl From<reqwest::Error> for BetterstackError {
    fn from(err: reqwest::Error) -> Self {
        BetterstackError::HttpError(err)
    }
}

pub trait BetterstackClientTrait: Send + Sync {
    fn put_logs<'a>(
        &'a self,
        dest: LogDestination,
        logs: Vec<LogEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), BetterstackError>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct BetterstackClient {
    http_client: reqwest::Client,
    source_token: String,
    ingestion_url: String,
}

impl BetterstackClient {
    pub fn new(source_token: impl Into<String>, ingestion_url: impl Into<String>) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            source_token: source_token.into(),
            ingestion_url: ingestion_url.into(),
        }
    }

    pub fn with_client(
        http_client: reqwest::Client,
        source_token: impl Into<String>,
        ingestion_url: impl Into<String>,
    ) -> Self {
        Self {
            http_client,
            source_token: source_token.into(),
            ingestion_url: ingestion_url.into(),
        }
    }
}

#[derive(Serialize)]
struct BetterstackEvent {
    message: String,
    dt: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

impl From<LogEvent> for BetterstackEvent {
    fn from(event: LogEvent) -> Self {
        Self {
            message: event.message,
            dt: event.timestamp.to_utc().timestamp_millis(),
            level: event.level,
            target: event.target,
            thread_id: event.thread_id,
            file: event.file,
            line: event.line,
        }
    }
}

impl BetterstackClientTrait for BetterstackClient {
    fn put_logs<'a>(
        &'a self,
        _: LogDestination,
        logs: Vec<LogEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), BetterstackError>> + Send + 'a>> {
        Box::pin(async move {
            if self.source_token.is_empty() {
                return Err(BetterstackError::InvalidConfig(
                    "Source token cannot be empty".into(),
                ));
            }

            let events: Vec<BetterstackEvent> = logs.into_iter().map(Into::into).collect();
            let body = serde_json::to_string(&events).map_err(|e| {
                BetterstackError::InvalidConfig(format!("Failed to serialize events: {}", e))
            })?;

            self.http_client
                .post(&self.ingestion_url)
                .header("Authorization", format!("Bearer {}", self.source_token))
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await?
                .error_for_status()?;

            Ok(())
        })
    }
}

pub struct NoopBetterstackClient;

impl BetterstackClientTrait for NoopBetterstackClient {
    fn put_logs<'a>(
        &'a self,
        _: LogDestination,
        _: Vec<LogEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), BetterstackError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

impl NoopBetterstackClient {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_client_empty_token() {
        let client = BetterstackClient::new("", "");
        let logs = vec![LogEvent::new("test".into())];

        let result = client.put_logs(LogDestination, logs).await;
        assert!(matches!(result, Err(BetterstackError::InvalidConfig(_))));
    }

    #[tokio::test]
    async fn test_noop_client() {
        let client = NoopBetterstackClient::new();
        let logs = vec![LogEvent::new("test".into())];

        let result = client.put_logs(LogDestination, logs).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_serialization() {
        let now = Utc::now();
        let event = LogEvent {
            message: "test message".into(),
            timestamp: now,
            level: Some("INFO".into()),
            target: Some("test_target".into()),
            thread_id: Some("ThreadId(1)".into()),
            file: Some("test.rs".into()),
            line: Some(42),
        };

        let betterstack_event: BetterstackEvent = event.into();
        assert_eq!(betterstack_event.message, "test message");
        assert_eq!(betterstack_event.dt, now.timestamp_millis());
        assert_eq!(betterstack_event.level, Some("INFO".into()));
        assert_eq!(betterstack_event.target, Some("test_target".into()));
        assert_eq!(betterstack_event.thread_id, Some("ThreadId(1)".into()));
        assert_eq!(betterstack_event.file, Some("test.rs".into()));
        assert_eq!(betterstack_event.line, Some(42));
    }

    #[test]
    fn test_client_new() {
        let client = BetterstackClient::new("token", "url");
        assert_eq!(client.source_token, "token");
        assert_eq!(client.ingestion_url, "url");
    }

    #[test]
    fn test_client_with_custom_http_client() {
        let http_client = reqwest::Client::new();
        let client = BetterstackClient::with_client(http_client, "token", "url");
        assert_eq!(client.source_token, "token");
        assert_eq!(client.ingestion_url, "url");
    }
}
