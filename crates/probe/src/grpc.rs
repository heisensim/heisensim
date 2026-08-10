use crate::config::GrpcProbeConfig;
use crate::http::ProbeResult;
use std::time::Instant;
use tonic::transport::Channel;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

/// Executes a gRPC health check probe using the standard `grpc.health.v1.Health/Check` protocol.
///
/// This implements the gRPC Health Checking Protocol as defined in:
/// <https://github.com/grpc/grpc/blob/master/doc/health-checking.md>
///
/// If `service` is `None`, checks the overall server health (empty service name).
/// If `service` is `Some(name)`, checks the health of the named service.
pub async fn check_grpc(config: &GrpcProbeConfig) -> ProbeResult {
    let start = Instant::now();
    let timeout = std::time::Duration::from_millis(config.timeout_ms);

    // Build the endpoint with timeout
    let endpoint = match Channel::from_shared(format!("http://{}", config.address)) {
        Ok(ep) => ep.timeout(timeout).connect_timeout(timeout),
        Err(e) => {
            return ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some(format!("Invalid gRPC address: {}", e)),
            };
        }
    };

    // Connect to the gRPC server
    let channel = match endpoint.connect().await {
        Ok(ch) => ch,
        Err(e) => {
            return ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some(format!("gRPC connection failed: {}", e)),
            };
        }
    };

    // Create health client and make the Check RPC
    let mut client = HealthClient::new(channel);
    let request = HealthCheckRequest {
        service: config.service.clone().unwrap_or_default(),
    };

    match client.check(request).await {
        Ok(response) => {
            let status = response.into_inner().status();
            let is_serving = status == ServingStatus::Serving;
            ProbeResult {
                success: is_serving,
                latency: start.elapsed(),
                status_code: Some(status as u16),
                error: if is_serving {
                    None
                } else {
                    Some(format!("gRPC health status: {:?}", status))
                },
            }
        }
        Err(e) => ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: Some(e.code() as u16),
            error: Some(format!("gRPC health check failed: {}", e)),
        },
    }
}
