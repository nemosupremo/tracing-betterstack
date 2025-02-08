use std::sync::Arc;

use tracing::{span, Event, Subscriber};
use tracing_subscriber::{
    fmt::{self, format, MakeWriter},
    layer::Context,
    registry::LookupSpan,
    Layer,
};

use crate::{
    client::BetterstackClient,
    dispatch::{BetterstackDispatcher, Dispatcher, NoopDispatcher},
    export::ExportConfig,
};

/// A Better Stack propagation layer.
pub struct BetterstackLayer<S, D, N = format::DefaultFields, E = format::Format<format::Full>> {
    fmt_layer: fmt::Layer<S, N, E, Arc<D>>,
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
        BetterstackLayer::new(Arc::new(NoopDispatcher::new()))
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
                .with_writer(dispatcher)
                .with_ansi(false)
                .with_level(true)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true),
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
    ) -> BetterstackLayer<S, BetterstackDispatcher, N, format::Format<L, T>> {
        let client = BetterstackClient::new(source_token, ingestion_url);
        BetterstackLayer {
            fmt_layer: self
                .fmt_layer
                .with_writer(Arc::new(BetterstackDispatcher::new(client, export_config))),
        }
    }

    /// Configure to display line number and filename.
    pub fn with_code_location(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_line_number(display).with_file(display),
        }
    }

    /// Configure to display target module path.
    pub fn with_target(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_target(display),
        }
    }

    /// Configure to display thread IDs.
    pub fn with_thread_ids(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_thread_ids(display),
        }
    }

    /// Configure to display thread names.
    pub fn with_thread_names(self, display: bool) -> Self {
        Self {
            fmt_layer: self.fmt_layer.with_thread_names(display),
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
        events: Arc<Mutex<Vec<String>>>,
    }

    impl TestDispatcher {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl Dispatcher for TestDispatcher {
        fn dispatch(&self, input: crate::dispatch::LogEvent) {
            self.events.lock().unwrap().push(input.message);
        }
    }

    impl std::io::Write for &TestDispatcher {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.events
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(buf).into_owned());
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
        assert!(events[0].contains("test message"));
        assert!(events[0].contains("INFO"));
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
        assert!(events[0].contains("test message in span"));
        assert!(events[0].contains("test_span"));
        assert!(events[0].contains("field=\"value\""));
    }

    #[test]
    fn test_layer_with_custom_formatting() {
        let dispatcher = Arc::new(TestDispatcher::new());
        let subscriber = tracing_subscriber::registry().with(
            BetterstackLayer::new(dispatcher.clone()).with_fmt_layer(
                fmt::Layer::default()
                    .without_time()
                    .with_target(false)
                    .with_level(true)
                    .event_format(fmt::format::Format::default().json()),
            ),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("test json message");
        });

        let events = dispatcher.events();
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("test json message"));
        assert!(events[0].contains("level"));
        assert!(events[0].starts_with('{'));
        assert!(events[0].ends_with("}\n"));
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
        assert!(events[0].contains("test message with location"));
        assert!(events[0].contains("layer.rs")); // Should contain file name
    }
}
