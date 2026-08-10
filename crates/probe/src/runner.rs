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

                            let event_kind = if result.success {
                                EventKind::ProbeSuccess {
                                    probe_name: name.clone(),
                                    latency_ms: result.latency.as_millis() as u64,
                                    status_code: result.status_code,
                                }
                            } else if result.error.as_deref().unwrap_or("").contains("timed out") || result.error.as_deref().unwrap_or("").contains("timeout") {
                                EventKind::ProbeTimeout {
                                    probe_name: name.clone(),
                                    timeout_ms: result.latency.as_millis() as u64,
                                }
                            } else {
                                EventKind::ProbeFailed {
                                    probe_name: name.clone(),
                                    error: result.error.clone().unwrap_or_default(),
                                    latency_ms: Some(result.latency.as_millis() as u64),
                                }
                            };

                            timeline.emit(event_kind);
                        }
                    }
                }
            });
        }

        Ok(())
    }
}
