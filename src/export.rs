use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::interval;

use crate::{
    client::{BetterstackClientTrait, NoopBetterstackClient},
    dispatch::LogEvent,
};

#[derive(Debug, Clone)]
pub struct ExportConfig {
    pub batch_size: usize,
    pub interval: Duration,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            interval: Duration::from_secs(5),
        }
    }
}

impl ExportConfig {
    pub fn with_batch_size(self, batch_size: usize) -> Self {
        Self { batch_size, ..self }
    }

    pub fn with_interval(self, interval: Duration) -> Self {
        Self { interval, ..self }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LogDestination;

pub(crate) struct BatchExporter<C> {
    client: C,
    queue: Vec<LogEvent>,
    config: ExportConfig,
}

impl Default for BatchExporter<NoopBetterstackClient> {
    fn default() -> Self {
        Self::new(NoopBetterstackClient::new(), ExportConfig::default())
    }
}

impl<C> BatchExporter<C> {
    pub(crate) fn new(client: C, config: ExportConfig) -> Self {
        let queue = Vec::with_capacity(config.batch_size);
        Self {
            client,
            config,
            queue,
        }
    }
}

impl<C> BatchExporter<C>
where
    C: BetterstackClientTrait + Send + Sync + 'static,
{
    pub(crate) async fn run(mut self, mut rx: UnboundedReceiver<LogEvent>) {
        let mut interval = interval(self.config.interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !self.queue.is_empty() {
                        self.flush_queue().await;
                    }
                }
                event = rx.recv() => {
                    match event {
                        Some(event) => {
                            self.queue.push(event);
                            if self.queue.len() >= self.config.batch_size {
                                self.flush_queue().await;
                            }
                        }
                        None => {
                            // Channel closed, flush remaining events
                            if !self.queue.is_empty() {
                                self.flush_queue().await;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn flush_queue(&mut self) {
        if let Err(err) = self
            .client
            .put_logs(LogDestination, std::mem::take(&mut self.queue))
            .await
        {
            eprintln!("[tracing-betterstack] Failed to send logs: {}", err);
        }
        self.queue.clear();
        self.queue.reserve(self.config.batch_size);
    }
}

#[cfg(test)]
mod tests {
    use crate::client::BetterstackError;

    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::mpsc;

    struct TestClient {
        received_logs: Arc<Mutex<Vec<LogEvent>>>,
    }

    impl TestClient {
        fn new() -> Self {
            Self {
                received_logs: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl BetterstackClientTrait for TestClient {
        fn put_logs<'a>(
            &'a self,
            _: LogDestination,
            logs: Vec<LogEvent>,
        ) -> Pin<Box<dyn Future<Output = Result<(), BetterstackError>> + Send + 'a>> {
            let received_logs = self.received_logs.clone();
            Box::pin(async move {
                received_logs.lock().unwrap().extend(logs);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn test_batch_exporter_sends_on_full_batch() {
        let client = TestClient::new();
        let received_logs = client.received_logs.clone();
        let config = ExportConfig {
            batch_size: 2,
            interval: Duration::from_secs(5),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let exporter = BatchExporter::new(client, config);

        let handle = tokio::spawn(exporter.run(rx));

        // Send events
        let event1 = LogEvent::new("test1".into());
        let event2 = LogEvent::new("test2".into());
        tx.send(event1).unwrap();
        tx.send(event2).unwrap();

        // Give some time for processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check received logs
        let logs = received_logs.lock().unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].message, "test1");
        assert_eq!(logs[1].message, "test2");

        // Cleanup
        drop(tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_batch_exporter_sends_on_interval() {
        let client = TestClient::new();
        let received_logs = client.received_logs.clone();
        let config = ExportConfig {
            batch_size: 10,                       // Larger than what we'll send
            interval: Duration::from_millis(100), // Short interval for testing
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let exporter = BatchExporter::new(client, config);

        let handle = tokio::spawn(exporter.run(rx));

        // Send one event
        let event = LogEvent::new("test".into());
        tx.send(event).unwrap();

        // Wait for interval to trigger
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Check received logs
        let logs = received_logs.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "test");

        // Cleanup
        drop(tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_batch_exporter_flushes_on_drop() {
        let client = TestClient::new();
        let received_logs = client.received_logs.clone();
        let config = ExportConfig {
            batch_size: 10,
            interval: Duration::from_secs(5),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let exporter = BatchExporter::new(client, config);

        let handle = tokio::spawn(exporter.run(rx));

        // Send an event
        let event = LogEvent::new("test".into());
        tx.send(event).unwrap();

        // Drop the sender to trigger flush
        drop(tx);
        let _ = handle.await;

        // Check that logs were flushed
        let logs = received_logs.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "test");
    }

    #[test]
    fn test_export_config() {
        let config = ExportConfig::default()
            .with_batch_size(50)
            .with_interval(Duration::from_secs(10));

        assert_eq!(config.batch_size, 50);
        assert_eq!(config.interval, Duration::from_secs(10));
    }
}
