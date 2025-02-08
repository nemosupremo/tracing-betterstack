use chrono::{DateTime, Utc};
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::{
    client::BetterstackClientTrait,
    export::{BatchExporter, ExportConfig},
};

pub trait Dispatcher {
    fn dispatch(&self, input: LogEvent);
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

impl LogEvent {
    pub fn new(message: String) -> Self {
        Self {
            message,
            timestamp: Utc::now(),
        }
    }
}

pub struct NoopDispatcher {}

impl Dispatcher for NoopDispatcher {
    fn dispatch(&self, _event: LogEvent) {}
}

impl NoopDispatcher {
    pub(crate) fn new() -> Self {
        Self {}
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
            eprintln!("[tracing-betterstack] Failed to dispatch log event: {}", err);
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
        let message = String::from_utf8_lossy(buf).to_string();
        self.dispatch(LogEvent::new(message));
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
}
