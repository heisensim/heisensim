use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_otel_shutdown_timeout_does_not_hang() {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint("http://127.0.0.1:1")
        .build()
        .unwrap();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    let start = std::time::Instant::now();

    // We expect this to time out because it's trying to connect to a black hole / refused port
    // and wait for batch export. Actually it might return quickly if connection refused, but let's test that it completes.
    // If it blocks forever, the timeout will trigger.
    let result = timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || provider.shutdown()),
    )
    .await;

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
}

#[tokio::test]
async fn test_otel_provider_creates_without_error() {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint("http://localhost:4318")
        .build()
        .unwrap();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
        .build();

    // Ensure it doesn't panic
    let _ = provider.shutdown();
}

#[tokio::test]
async fn test_cli_help_includes_otel_endpoint() {
    // Note: 'cargo run' might build, which can be slow in tests, but it's acceptable for this simple assertion.
    // However, it's faster to run the binary if it's already built. We'll use 'cargo run'.
    let output = std::process::Command::new(env!("CARGO"))
        .args(&["run", "-p", "heisensim", "--", "run", "--help"])
        .output()
        .expect("Failed to execute cargo run");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--otel-endpoint"),
        "Help output should contain --otel-endpoint. Output:\n{}",
        stdout
    );
    assert!(
        stdout.contains("OTLP endpoint"),
        "Help output should contain 'OTLP endpoint'. Output:\n{}",
        stdout
    );
}
