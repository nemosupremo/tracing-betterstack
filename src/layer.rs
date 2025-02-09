use std::sync::Arc;

use chrono::Utc;
use tracing::{span, Event, Subscriber};
use tracing_subscriber::{
    fmt::{self, format, MakeWriter},
    layer::Context,
    registry::LookupSpan,
    Layer,
};

use crate::{
    client::BetterstackClient,
    dispatch::{BetterstackDispatcher, Dispatcher, LogEvent, NoopDispatcher},
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
pub struct BetterstackLayer<S, D, N = format::DefaultFields, E = format::Format<format::Full>> {
    fmt_layer: fmt::Layer<S, N, E, Arc<D>>,
    dispatcher: Arc<D>,
}

/// Construct BetterstackLayer to compose with tracing subscriber.
pub fn layer<S>() -> BetterstackLayer<S, NoopDispatcher>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    BetterstackLayer::default()
}

impl<S> Default for BetterstackLayer<S, NoopDispatcher>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn default() -> Self {
        let dispatcher = Arc::new(NoopDispatcher::new());
        BetterstackLayer::new(dispatcher)
    }
}

impl<S, D> BetterstackLayer<S, D>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    D: Dispatcher + 'static,
    Arc<D>: for<'writer> MakeWriter<'writer>,
{
    fn new(dispatcher: Arc<D>) -> Self {
        Self {
            fmt_layer: fmt::Layer::default()
                .with_writer(dispatcher.clone())
                .with_ansi(false)
                .with_level(true)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true),
            dispatcher,
        }
    }
}

impl<S, D, N, L, T> BetterstackLayer<S, D, N, format::Format<L, T>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    D: Dispatcher + 'static,
    Arc<D>: for<'writer> MakeWriter<'writer>,
    N: for<'writer> format::FormatFields<'writer> + 'static,
{
    /// Set the client using a source token and export configuration.
    pub fn with_client(
        self,
        source_token: impl Into<String>,
        ingestion_url: impl Into<String>,
        export_config: ExportConfig,
    ) -> BetterstackLayer<S, BetterstackDispatcher, N, format::Format<L, T>>
    where
        BetterstackDispatcher: Dispatcher,
    {
        let client = BetterstackClient::new(source_token, ingestion_url);
        let dispatcher = Arc::new(BetterstackDispatcher::new(client, export_config));
        BetterstackLayer {
            fmt_layer: self.fmt_layer.with_writer(dispatcher.clone()),
            dispatcher,
        }
    }

    /// Configure to display line number and filename.
    pub fn with_code_location(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_line_number(display).with_file(display),
            dispatcher: self.dispatcher,
        }
    }

    /// Configure to display target module path.
    pub fn with_target(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_target(display),
            dispatcher: self.dispatcher,
        }
    }

    /// Configure to display thread IDs.
    pub fn with_thread_ids(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_thread_ids(display),
            dispatcher: self.dispatcher,
        }
    }

    /// Configure to display thread names.
    pub fn with_thread_names(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_thread_names(display),
            dispatcher: self.dispatcher,
        }
    }

    /// Set a custom format layer for log formatting.
    pub fn with_fmt_layer<N2, E2, W>(
        self,
        fmt_layer: fmt::Layer<S, N2, E2, W>,
    ) -> BetterstackLayer<S, D, N2, E2> {
        let writer = self.fmt_layer.writer().clone();
        BetterstackLayer {
            fmt_layer: fmt_layer.with_writer(writer),
            dispatcher: self.dispatcher,
        }
    }
}

impl<S, D, N, E> Layer<S> for BetterstackLayer<S, D, N, E>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    D: Dispatcher + 'static,
    Arc<D>: for<'writer> MakeWriter<'writer>,
    N: for<'writer> format::FormatFields<'writer> + 'static,
    E: format::FormatEvent<S, N> + 'static,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // Extract message using visitor
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Create structured log event
        let metadata = event.metadata();
        let log_event = LogEvent {
            message: visitor.0,
            timestamp: Utc::now(),
            level: Some(metadata.level().to_string()),
            target: Some(metadata.target().to_string()),
            thread_id: Some(format!(
                "{:?}",
                std::thread::current().id()
            )),
            file: metadata.file().map(String::from),
            line: metadata.line(),
        };

        // Dispatch the event
        self.dispatcher.dispatch(log_event);

        // Also handle normal formatting
        self.fmt_layer.on_event(event, ctx)
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_enter(id, ctx)
    }

    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_exit(id, ctx)
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_close(id, ctx)
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_new_span(attrs, id, ctx)
    }

    fn on_record(&self, span: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        self.fmt_layer.on_record(span, values, ctx)
    }

    fn enabled(&self, metadata: &tracing::Metadata<'_>, ctx: Context<'_, S>) -> bool {
        self.fmt_layer.enabled(metadata, ctx)
    }
}

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

    impl std::io::Write for &TestDispatcher {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_layer_basic_logging() {
        let dispatcher = Arc::new(TestDispatcher::new());
        let subscriber = tracing_subscriber::registry().with(
            BetterstackLayer::new(dispatcher.clone())
                .with_code_location(false)
                .with_target(false),
        );

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
        let subscriber = tracing_subscriber::registry().with(
            BetterstackLayer::new(dispatcher.clone())
                .with_code_location(false)
                .with_target(false),
        );

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

    #[test]
    fn test_layer_with_code_location() {
        let dispatcher = Arc::new(TestDispatcher::new());
        let subscriber = tracing_subscriber::registry().with(
            BetterstackLayer::new(dispatcher.clone())
                .with_code_location(true)
                .with_target(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("test message with location");
        });

        let events = dispatcher.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "test message with location");
        assert!(events[0].file.is_some());
        assert!(events[0].line.is_some());
    }
}
