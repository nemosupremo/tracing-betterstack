use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::runtime::Handle;

use crate::{
    client::{BetterstackClientTrait, BetterstackEvent},
    export::{BatchExporter, ExportConfig},
};

pub trait Dispatcher: Send + Sync {
    fn dispatch(&self, input: LogEvent);
}

#[derive(Debug)]
pub enum LogMessage {
    Event(LogEvent),
    Shutdown(flume::Sender<()>),
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub fields: LogFields,
}

#[derive(Debug, Clone, Default)]
pub struct LogFields(
    pub HashMap<String, serde_json::Value, BuildHasherDefault<seahash::SeaHasher>>,
);

impl tracing::field::Visit for LogFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::String(format!("{:?}", value)),
        );
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(number));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if let Some(number) = serde_json::Number::from_i128(value as _) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(number));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if let Some(number) = serde_json::Number::from_u128(value as _) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(number));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        if let Some(number) = serde_json::Number::from_i128(value) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(number));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        if let Some(number) = serde_json::Number::from_u128(value) {
            self.0
                .insert(field.name().to_string(), serde_json::Value::Number(number));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(
            field.name().to_string(),
            serde_json::Value::String(value.to_string()),
        );
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
    inner: Arc<BetterstackDispatcherInner>,
}

struct BetterstackDispatcherInner {
    tx: flume::Sender<LogMessage>,
    // Fallback queue for when async runtime is not available
    sync_queue: Mutex<Vec<LogMessage>>,
}

pub struct WorkerGuard {
    tx: flume::Sender<LogMessage>,
}

impl BetterstackDispatcher {
    pub(crate) fn new<C, F: Fn(&mut BetterstackEvent) + Send + Sync + 'static>(
        client: C,
        export_config: ExportConfig<F>,
    ) -> (Self, WorkerGuard)
    where
        C: BetterstackClientTrait + 'static,
    {
        let (tx, rx) = flume::unbounded(); // maybe make bounded?
        let exporter = BatchExporter::new(client, export_config);

        // Try to get the current Tokio runtime handle
        let runtime_available = Handle::try_current().is_ok();

        let inner = Arc::new(BetterstackDispatcherInner {
            tx: tx.clone(),
            sync_queue: Mutex::new(Vec::new()),
        });

        if runtime_available {
            // We're in an async context, spawn the exporter
            tokio::spawn(exporter.run(rx));
        } else {
            // We're in a sync context, create a new runtime for the exporter
            std::thread::spawn({
                let inner = inner.clone();
                move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        // Process any messages that were queued before the runtime was available
                        let queued = {
                            let mut queue = inner.sync_queue.lock().unwrap();
                            std::mem::take(&mut *queue)
                        };
                        for event in queued {
                            let _ = inner.tx.send(event);
                        }

                        exporter.run(rx).await;
                    });
                }
            });
        }

        (Self { inner }, WorkerGuard { tx })
    }
}

impl Dispatcher for BetterstackDispatcher {
    fn dispatch(&self, event: LogEvent) {
        match self.inner.tx.send(LogMessage::Event(event.clone())) {
            Ok(_) => {}
            Err(_) => {
                // If sending fails, store in sync queue
                let mut queue = self.inner.sync_queue.lock().unwrap();
                queue.push(LogMessage::Event(event))
            }
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

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        use std::time::Duration;

        use flume::SendTimeoutError;

        let (tx, rx) = flume::bounded(1);
        match self
            .tx
            .send_timeout(LogMessage::Shutdown(tx), Duration::from_millis(100))
        {
            Ok(_) => {
                // Attempt to wait for `Worker` to flush all messages before dropping. This happens
                // when the `Worker` calls `recv()` on a zero-capacity channel. Use `send_timeout`
                // so that drop is not blocked indefinitely.
                // TODO: Make timeout configurable.
                let _ = rx.recv_timeout(Duration::from_millis(2000));
            }
            Err(SendTimeoutError::Disconnected(_)) => (),
            Err(SendTimeoutError::Timeout(e)) => eprintln!(
                "Failed to send shutdown signal to logging worker. Error: {:?}",
                e
            ),
        }
    }
}

/*
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
    use crate::client::NoopBetterstackClient;
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn test_sync_context() {
        let export_config = ExportConfig {
            batch_size: 1,
            interval: Duration::from_millis(100),
        };

        let dispatcher = BetterstackDispatcher::new(NoopBetterstackClient::new(), export_config);
        let event = LogEvent::new("test message".into());

        // This should work even outside of an async context
        dispatcher.dispatch(event);

        let mut dispatcher_ref = &dispatcher;
        assert!(dispatcher_ref.write_all(b"test message").is_ok());
        assert!(dispatcher_ref.flush().is_ok());
    }

    #[tokio::test]
    async fn test_async_context() {
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

        // Give some time for async processing
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
 */
