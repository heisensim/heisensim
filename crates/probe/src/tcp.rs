use crate::config::TcpProbeConfig;
use crate::http::ProbeResult;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Executes a TCP health probe.
pub async fn check_tcp(config: &TcpProbeConfig) -> ProbeResult {
    let start = Instant::now();
    let timeout_dur = Duration::from_millis(config.timeout_ms);
    let addr = format!("{}:{}", config.host, config.port);

    match timeout(timeout_dur, TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => ProbeResult {
            success: true,
            latency: start.elapsed(),
            status_code: None,
            error: None,
        },
        Ok(Err(e)) => ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: None,
            error: Some(format!("Connection failed: {}", e)),
        },
        Err(_) => ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: None,
            error: Some("Connection timed out".to_string()),
        },
    }
}
