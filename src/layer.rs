use chrono::Utc;
use std::sync::Arc;
use tracing::{span, Event, Subscriber};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

use crate::{
    client::{BetterstackClient, BetterstackEvent},
    dispatch::{
        BetterstackDispatcher, Dispatcher, LogEvent, LogFields, NoopDispatcher, WorkerGuard,
    },
    export::ExportConfig,
};

#[derive(Default)]
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value).trim_matches('"').to_string();
        }
    }
}

/// A Better Stack propagation layer.
pub struct BetterstackLayer<S> {
    dispatcher: Arc<dyn Dispatcher>,
    _subscriber: std::marker::PhantomData<S>,
}

/// Construct BetterstackLayer to compose with tracing subscriber.
pub fn layer<S>() -> BetterstackLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    BetterstackLayer::default()
}

impl<S> Default for BetterstackLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn default() -> Self {
        let dispatcher = Arc::new(NoopDispatcher::new());
        BetterstackLayer::new(dispatcher)
    }
}

impl<S> BetterstackLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn new(dispatcher: Arc<dyn Dispatcher>) -> Self {
        Self {
            dispatcher,
            _subscriber: std::marker::PhantomData,
        }
    }

    /// Set the client using a source token and export configuration.
    pub fn with_client<F: Fn(&mut BetterstackEvent) + Send + Sync + 'static>(
        self,
        source_token: impl Into<String>,
        ingestion_url: impl Into<String>,
        mut export_config: ExportConfig<F>,
    ) -> (BetterstackLayer<S>, WorkerGuard)
    where
        BetterstackDispatcher: Dispatcher,
    {
        let client = BetterstackClient::new(
            source_token,
            ingestion_url,
            export_config
                .transform
                .take()
                .map(|t| std::sync::Arc::new(t)),
        );
        let (dispatcher, guard) = BetterstackDispatcher::new(client, export_config);
        let dispatcher = Arc::new(dispatcher);
        (BetterstackLayer::new(dispatcher), guard)
    }
}

impl<S> Layer<S> for BetterstackLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        // Extract message using visitor
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Create structured log event
        let metadata = event.metadata();
        let mut fields = LogFields::default();
        event.record(&mut fields);
        let log_event = LogEvent {
            message: visitor.0,
            timestamp: Utc::now(),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            file: metadata.file().map(String::from),
            line: metadata.line(),
            fields,
        };

        // Dispatch the event
        self.dispatcher.dispatch(log_event);
    }

    fn on_enter(&self, _: &span::Id, _: Context<'_, S>) {}
    fn on_exit(&self, _: &span::Id, _: Context<'_, S>) {}
    fn on_close(&self, _: span::Id, _: Context<'_, S>) {}
    fn on_new_span(&self, _: &span::Attributes<'_>, _: &span::Id, _: Context<'_, S>) {}
    fn on_record(&self, _: &span::Id, _: &span::Record<'_>, _: Context<'_, S>) {}

    fn enabled(&self, metadata: &tracing::Metadata<'_>, _: Context<'_, S>) -> bool {
        metadata.is_event()
    }
}
/*
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tracing::{span, Level};
    use tracing_subscriber::layer::SubscriberExt;

    struct TestDispatcher {
        events: Arc<Mutex<Vec<LogEvent>>>,
    }

    impl TestDispatcher {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn events(&self) -> Vec<LogEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl Dispatcher for TestDispatcher {
        fn dispatch(&self, input: LogEvent) {
            self.events.lock().unwrap().push(input);
        }
    }

    #[test]
    fn test_layer_basic_logging() {
        let dispatcher = Arc::new(TestDispatcher::new());
        let subscriber =
            tracing_subscriber::registry().with(BetterstackLayer::new(dispatcher.clone()));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("test message");
        });

        let events = dispatcher.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "test message");
        assert_eq!(events[0].level.as_deref(), Some("INFO"));
    }

    #[test]
    fn test_layer_with_spans() {
        let dispatcher = Arc::new(TestDispatcher::new());
        let subscriber =
            tracing_subscriber::registry().with(BetterstackLayer::new(dispatcher.clone()));

        tracing::subscriber::with_default(subscriber, || {
            let span = span!(Level::INFO, "test_span", field = "value");
            let _guard = span.enter();
            tracing::info!("test message in span");
        });

        let events = dispatcher.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "test message in span");
        assert_eq!(events[0].level.as_deref(), Some("INFO"));
    }
}
    */
