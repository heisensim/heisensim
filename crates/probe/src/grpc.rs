use crate::config::GrpcProbeConfig;
use crate::http::ProbeResult;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Executes a gRPC health check probe.
pub async fn check_grpc(config: &GrpcProbeConfig) -> ProbeResult {
    // TODO: Implement full `grpc.health.v1.Health/Check` support when tonic-health client is integrated.
    // For now, this acts as a placeholder that performs a TCP connection check to the gRPC address.
    
    let start = Instant::now();
    let timeout_dur = Duration::from_millis(config.timeout_ms);

    match timeout(timeout_dur, TcpStream::connect(&config.address)).await {
        Ok(Ok(_stream)) => {
            ProbeResult {
                success: true,
                latency: start.elapsed(),
                status_code: None,
                error: None,
            }
        }
        Ok(Err(e)) => {
            ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some(format!("TCP connection to gRPC address failed: {}", e)),
            }
        }
        Err(_) => {
            ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some("gRPC TCP connection timed out".to_string()),
            }
        }
    }
}
