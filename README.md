# Tracing for Better Stack (WIP)

This library is a work in progress and is not yet ready for production use.

### TODO
 - [ ] Fix spans not being sent
 - [ ] Fix logs not flushing on exit

## Overview

This library is designed to provide a layer to for [tracing](https://github.com/tokio-rs/tracing) that allows for logs to be streamed to Better Stack / Logtail.

## Usage

```rust
// Get credentials from environment
let source_token = std::env::var("BETTERSTACK_SOURCE_TOKEN")
  .expect("BETTERSTACK_SOURCE_TOKEN must be set");

let ingestion_url = std::env::var("BETTERSTACK_INGEST_URL")
  .expect("BETTERSTACK_INGEST_URL must be set");

// Create Betterstack layer
let betterstack_layer = layer()
  .with(tracing_subscriber::filter::filter_fn(|metadata| {
    metadata.target().starts_with("tracing_betterstack")
  }))
  .with_client(
    source_token,
    ingestion_url,
    ExportConfig::default()
      .with_batch_size(100)
      .with_interval(Duration::from_secs(5))
  )
  .with_code_location(true)
  .with_target(true)
  .with_thread_ids(true);

  tracing_subscriber::registry()
    .with(betterstack_layer.with_filter(filter))
    .init();
```
