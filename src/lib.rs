mod client;
mod dispatch;
mod export;
mod layer;

pub use client::{BetterstackClient, BetterstackClientTrait, BetterstackError};
pub use export::{ExportConfig, LogDestination};
pub use layer::{layer, BetterstackLayer};

/*
#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, time::Duration};
    use tracing_subscriber::{
        fmt::{self, format::FmtSpan},
        layer::SubscriberExt,
        util::SubscriberInitExt,
    };

    fn init_env() -> Result<(String, String), Box<dyn std::error::Error>> {
        dotenv::dotenv().ok();

        let token =
            env::var("BETTERSTACK_SOURCE_TOKEN").map_err(|_| "BETTERSTACK_SOURCE_TOKEN not set")?;
        let url =
            env::var("BETTERSTACK_INGEST_URL").map_err(|_| "BETTERSTACK_INGEST_URL not set")?;

        Ok((token, url))
    }

    fn init_subscriber(token: String, url: String) -> impl tracing::Subscriber {
        tracing_subscriber::registry()
            .with(tracing_subscriber::filter::filter_fn(|metadata| {
                metadata.target().starts_with("tracing_betterstack")
            }))
            .with(layer().with_client(token, url, ExportConfig::default()))
            .with(fmt::Layer::new().with_span_events(FmtSpan::CLOSE))
    }

    #[test]
    fn test_export_config() {
        let config = ExportConfig::default()
            .with_batch_size(50)
            .with_interval(Duration::from_secs(10));

        assert_eq!(config.batch_size, 50);
        assert_eq!(config.interval, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_basic_initialization() {
        let (token, url) = match init_env() {
            Ok(env_vars) => env_vars,
            Err(e) => {
                eprintln!("Skipping test: {}", e);
                return;
            }
        };

        let subscriber = init_subscriber(token, url);

        let _guard = subscriber.set_default();
        tracing::info!(target: "tracing_betterstack::test", "Test log message");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn test_custom_formatting() {
        let (token, url) = match init_env() {
            Ok(env_vars) => env_vars,
            Err(e) => {
                eprintln!("Skipping test: {}", e);
                return;
            }
        };

        let subscriber = init_subscriber(token, url);
        let _guard = subscriber.set_default();

        let span = tracing::info_span!(
            target: "tracing_betterstack::test",
            "test_span",
            field = "value"
        );

        let _span_guard = span.enter();
        tracing::info!(target: "tracing_betterstack::test", "Test log message with custom format");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn test_batch_behavior() {
        let (token, url) = match init_env() {
            Ok(env_vars) => env_vars,
            Err(e) => {
                eprintln!("Skipping test: {}", e);
                return;
            }
        };

        let subscriber = init_subscriber(token, url);
        let _guard = subscriber.set_default();

        tracing::info!(target: "tracing_betterstack::test", "Batch test message 1");
        tracing::info!(target: "tracing_betterstack::test", "Batch test message 2");

        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    #[tokio::test]
    async fn test_interval_flush() {
        let (token, url) = match init_env() {
            Ok(env_vars) => env_vars,
            Err(e) => {
                eprintln!("Skipping test: {}", e);
                return;
            }
        };

        let subscriber = init_subscriber(token, url);
        let _guard = subscriber.set_default();

        tracing::info!(target: "tracing_betterstack::test", "Interval flush test message");

        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
*/
