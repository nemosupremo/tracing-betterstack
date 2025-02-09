use chrono::{DateTime, Utc};
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::{
    client::BetterstackClientTrait,
    export::{BatchExporter, ExportConfig},
};

pub trait Dispatcher: Send + Sync {
    fn dispatch(&self, input: LogEvent);
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub level: Option<String>,
    pub target: Option<String>,
    pub thread_id: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

impl LogEvent {
    pub fn new(message: String) -> Self {
        Self {
            message,
            timestamp: Utc::now(),
            level: None,
            target: None,
            thread_id: None,
            file: None,
            line: None,
        }
    }

    pub fn with_metadata(
        message: String,
        level: Option<String>,
        target: Option<String>,
        thread_id: Option<String>,
        file: Option<String>,
        line: Option<u32>,
    ) -> Self {
        Self {
            message,
            timestamp: Utc::now(),
            level,
            target,
            thread_id,
            file,
            line,
        }
    }
}

pub struct NoopDispatcher;

impl Dispatcher for NoopDispatcher {
    fn dispatch(&self, _event: LogEvent) {}
}

impl NoopDispatcher {
    pub(crate) fn new() -> Self {
        Self
    }
}

pub struct BetterstackDispatcher {
    tx: UnboundedSender<LogEvent>,
}

impl BetterstackDispatcher {
    pub(crate) fn new<C>(client: C, export_config: ExportConfig) -> Self
    where
        C: BetterstackClientTrait + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let exporter = BatchExporter::new(client, export_config);

        tokio::spawn(async move {
            exporter.run(rx).await;
        });

        Self { tx }
    }
}

impl Dispatcher for BetterstackDispatcher {
    fn dispatch(&self, event: LogEvent) {
        if let Err(err) = self.tx.send(event) {
            eprintln!(
                "[tracing-betterstack] Failed to dispatch log event: {}",
                err
            );
        }
    }
}

impl std::io::Write for &NoopDispatcher {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Write for &BetterstackDispatcher {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_noop_dispatcher() {
        let dispatcher = NoopDispatcher::new();
        let event = LogEvent::new("test message".into());
        dispatcher.dispatch(event.clone());

        let mut dispatcher_ref = &dispatcher;
        assert!(dispatcher_ref.write_all(b"test message").is_ok());
        assert!(dispatcher_ref.flush().is_ok());
    }

    #[tokio::test]
    async fn test_betterstack_dispatcher() {
        use crate::client::NoopBetterstackClient;
        use std::time::Duration;

        let export_config = ExportConfig {
            batch_size: 1,
            interval: Duration::from_millis(100),
        };

        let dispatcher = BetterstackDispatcher::new(NoopBetterstackClient::new(), export_config);
        let event = LogEvent::new("test message".into());

        dispatcher.dispatch(event);

        let mut dispatcher_ref = &dispatcher;
        assert!(dispatcher_ref.write_all(b"test message").is_ok());
        assert!(dispatcher_ref.flush().is_ok());
    }

    #[test]
    fn test_log_event_new() {
        let message = "test message";
        let event = LogEvent::new(message.to_string());

        assert_eq!(event.message, message);
        assert!(event.level.is_none());
        assert!(event.target.is_none());
        assert!(event.thread_id.is_none());
        assert!(event.file.is_none());
        assert!(event.line.is_none());
    }

    #[test]
    fn test_log_event_with_metadata() {
        let message = "test message";
        let level = Some("INFO".to_string());
        let target = Some("test_target".to_string());
        let thread_id = Some("ThreadId(1)".to_string());
        let file = Some("test.rs".to_string());
        let line = Some(42);

        let event = LogEvent::with_metadata(
            message.to_string(),
            level.clone(),
            target.clone(),
            thread_id.clone(),
            file.clone(),
            line,
        );

        assert_eq!(event.message, message);
        assert_eq!(event.level, level);
        assert_eq!(event.target, target);
        assert_eq!(event.thread_id, thread_id);
        assert_eq!(event.file, file);
        assert_eq!(event.line, line);
    }
}
