use std::collections::HashMap;
use std::future::Future;
use std::hash::BuildHasherDefault;
use std::pin::Pin;

use serde::Serialize;

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

#[derive(Clone)]
pub struct BetterstackClient<F: Fn(&mut BetterstackEvent) + Send + Sync + 'static> {
    http_client: reqwest::Client,
    source_token: String,
    ingestion_url: String,
    transform: Option<std::sync::Arc<F>>,
}

impl<F> BetterstackClient<F>
where
    F: Fn(&mut BetterstackEvent) + Send + Sync + 'static,
{
    pub fn new(
        source_token: impl Into<String>,
        ingestion_url: impl Into<String>,
        transform: Option<std::sync::Arc<F>>,
    ) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            source_token: source_token.into(),
            ingestion_url: ingestion_url.into(),
            transform,
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
            transform: None,
        }
    }
}

#[derive(Serialize)]
pub struct BetterstackEvent {
    message: String,
    dt: String,
    level: String,
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,

    #[serde(flatten)]
    fields: HashMap<String, serde_json::Value, BuildHasherDefault<seahash::SeaHasher>>,
}

impl BetterstackEvent {
    pub fn add_field_str(&mut self, name: &str, value: &str) {
        self.fields.insert(
            name.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

impl From<LogEvent> for BetterstackEvent {
    fn from(event: LogEvent) -> Self {
        Self {
            message: event.message,
            dt: event
                .timestamp
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            level: event.level,
            target: event.target,
            file: event.file,
            line: event.line,
            fields: event.fields.0,
        }
    }
}

impl<F> BetterstackClientTrait for BetterstackClient<F>
where
    F: Fn(&mut BetterstackEvent) + Send + Sync + 'static,
{
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

            let events: Vec<BetterstackEvent> = logs
                .into_iter()
                .map(Into::into)
                .map(|mut e: BetterstackEvent| {
                    if let Some(transform) = self.transform.as_ref() {
                        transform(&mut e);
                    }
                    e
                })
                .collect();
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

/*
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
    */
