use crate::config::{HttpMethod, HttpProbeConfig};
use opentelemetry::global;
use reqwest::{Client, Method};
use std::time::{Duration, Instant};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// The result of executing a health probe.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    /// Whether the probe was considered successful.
    pub success: bool,
    /// The time taken to execute the probe.
    pub latency: Duration,
    /// The HTTP status code, if applicable.
    pub status_code: Option<u16>,
    /// Any error message encountered during execution.
    pub error: Option<String>,
}

/// Executes an HTTP health probe.
pub async fn check_http(config: &HttpProbeConfig) -> ProbeResult {
    check_http_inner(config)
        .instrument(tracing::info_span!(
            "probe.http",
            probe.name = %config.name,
            http.url = %config.url,
            http.method = ?config.method,
        ))
        .await
}

async fn check_http_inner(config: &HttpProbeConfig) -> ProbeResult {
    let start = Instant::now();
    let timeout = Duration::from_millis(config.timeout_ms);

    let client = match Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(e) => {
            return ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some(format!("Failed to build HTTP client: {}", e)),
            };
        }
    };

    let method = match config.method {
        HttpMethod::Get => Method::GET,
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
    };

    let mut req = client.request(method, &config.url);
    if let Some(headers) = &config.headers {
        for (k, v) in headers {
            req = req.header(k, v);
        }
    }

    let cx = tracing::Span::current().context();
    let mut injector_headers = std::collections::HashMap::new();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut injector_headers);
    });
    for (key, value) in &injector_headers {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                req = req.header(header_name, header_value);
            }
        }
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let success = status == config.expected_status;
            ProbeResult {
                success,
                latency: start.elapsed(),
                status_code: Some(status),
                error: if success {
                    None
                } else {
                    Some(format!("Unexpected status: {}", status))
                },
            }
        }
        Err(e) => ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: e.status().map(|s| s.as_u16()),
            error: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_result_creation() {
        let result = ProbeResult {
            success: true,
            latency: Duration::from_millis(50),
            status_code: Some(200),
            error: None,
        };
        assert!(result.success);
        assert_eq!(result.status_code, Some(200));
    }

    #[tokio::test]
    #[ignore = "Requires network access"]
    async fn test_check_http_live() {
        let config = HttpProbeConfig {
            name: "test-get".to_string(),
            url: "https://httpbin.org/status/200".to_string(),
            method: HttpMethod::Get,
            expected_status: 200,
            timeout_ms: 5000,
            interval_ms: 1000,
            headers: None,
        };

        let result = check_http(&config).await;
        assert!(result.success, "HTTP probe failed: {:?}", result.error);
        assert_eq!(result.status_code, Some(200));
    }
}
