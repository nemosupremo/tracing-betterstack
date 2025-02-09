use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;
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
    tx: UnboundedSender<LogEvent>,
    // Fallback queue for when async runtime is not available
    sync_queue: Mutex<Vec<LogEvent>>,
}

impl BetterstackDispatcher {
    pub(crate) fn new<C>(client: C, export_config: ExportConfig) -> Self
    where
        C: BetterstackClientTrait + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let exporter = BatchExporter::new(client, export_config);

        // Try to get the current Tokio runtime handle
        let runtime_available = Handle::try_current().is_ok();

        let inner = Arc::new(BetterstackDispatcherInner {
            tx,
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

        Self { inner }
    }
}

impl Dispatcher for BetterstackDispatcher {
    fn dispatch(&self, event: LogEvent) {
        match self.inner.tx.send(event.clone()) {
            Ok(_) => {}
            Err(_) => {
                // If sending fails, store in sync queue
                let mut queue = self.inner.sync_queue.lock().unwrap();
                queue.push(event);
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
