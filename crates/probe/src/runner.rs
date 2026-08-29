use crate::config::ProbeConfig;
use crate::exec::check_exec;
use crate::grpc::check_grpc;
use crate::http::check_http;
use crate::tcp::check_tcp;
use heisensim_timeline::{EventKind, TimelineHandle};
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::info;

/// Orchestrates the execution of multiple health probes.
pub struct ProbeRunner {
    probes: Vec<ProbeConfig>,
    timeline: TimelineHandle,
}

impl ProbeRunner {
    /// Creates a new `ProbeRunner`.
    pub fn new(probes: Vec<ProbeConfig>, timeline: TimelineHandle) -> Self {
        Self { probes, timeline }
    }

    /// Spawns the probes and runs them concurrently until a cancellation signal is received.
    pub fn run(&self, cancel: watch::Receiver<bool>) -> anyhow::Result<()> {
        for probe in &self.probes {
            let probe = probe.clone();
            let timeline = self.timeline.clone();
            let mut cancel = cancel.clone();

            tokio::spawn(async move {
                let name = probe.name().to_string();

                loop {
                    let interval = probe.interval();
                    let span = tracing::info_span!(
                        "probe.cycle",
                        probe.name = %name,
                        probe.interval_ms = interval.as_millis() as u64,
                    );
                    let _guard = span.enter();

                    tokio::select! {
                        _ = cancel.changed() => {
                            if *cancel.borrow() {
                                info!("Probe {} cancelled", name);
                                break;
                            }
                        }
                        _ = sleep(probe.interval()) => {
                            let result = match &probe {
                                ProbeConfig::Http(c) => check_http(c).await,
                                ProbeConfig::Tcp(c) => check_tcp(c).await,
                                ProbeConfig::Grpc(c) => check_grpc(c).await,
                                ProbeConfig::Exec(c) => check_exec(c).await,
                            };

                            let (event_kind, status_label) = if result.success {
                                (EventKind::ProbeSuccess {
                                    probe_name: name.clone(),
                                    latency_ms: result.latency.as_millis() as u64,
                                    status_code: result.status_code,
                                }, "success")
                            } else if result.error.as_deref().unwrap_or("").contains("timed out") || result.error.as_deref().unwrap_or("").contains("timeout") {
                                (EventKind::ProbeTimeout {
                                    probe_name: name.clone(),
                                    timeout_ms: result.latency.as_millis() as u64,
                                }, "timeout")
                            } else {
                                (EventKind::ProbeFailed {
                                    probe_name: name.clone(),
                                    error: result.error.clone().unwrap_or_default(),
                                    latency_ms: Some(result.latency.as_millis() as u64),
                                }, "failure")
                            };

                            // Streaming OTel metrics (no-op if no --otel-endpoint)
                            let meter = opentelemetry::global::meter("heisensim");
                            let latency_hist = meter
                                .f64_histogram("heisensim.probe.latency_ms")
                                .with_description("Probe latency in milliseconds")
                                .build();
                            let probe_counter = meter
                                .u64_counter("heisensim.probe.count")
                                .with_description("Probe execution count by status")
                                .build();

                            let attrs = [
                                opentelemetry::KeyValue::new("probe", name.clone()),
                                opentelemetry::KeyValue::new("status", status_label),
                            ];
                            latency_hist.record(result.latency.as_secs_f64() * 1000.0, &attrs);
                            probe_counter.add(1, &attrs);

                            timeline.emit(event_kind);
                        }
                    }
                }
            });
        }

        Ok(())
    }
}
