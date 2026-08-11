use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider;

pub fn emit_verdict_metrics(
    meter_provider: &opentelemetry_sdk::metrics::SdkMeterProvider,
    verdicts: &[heisensim_props::PropertyVerdict],
    seed: u64,
    duration_secs: f64,
    total_faults: usize,
) {
    let meter = meter_provider.meter("heisensim");

    // Property pass/fail counters
    let property_counter = meter.u64_counter("heisensim.property.checks").build();
    let mut passed = 0u64;
    let mut failed = 0u64;
    for v in verdicts {
        let attrs = vec![
            KeyValue::new("property", v.property_name.clone()),
            KeyValue::new("result", if v.passed { "pass" } else { "fail" }),
        ];
        property_counter.add(1, &attrs);
        if v.passed {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    // Summary gauges
    let pass_gauge = meter.u64_gauge("heisensim.properties.passed").build();
    let fail_gauge = meter.u64_gauge("heisensim.properties.failed").build();
    pass_gauge.record(passed, &[KeyValue::new("seed", seed.to_string())]);
    fail_gauge.record(failed, &[KeyValue::new("seed", seed.to_string())]);

    // Run metrics
    let duration_hist = meter
        .f64_histogram("heisensim.run.duration_seconds")
        .build();
    duration_hist.record(duration_secs, &[KeyValue::new("seed", seed.to_string())]);

    let faults_counter = meter.u64_counter("heisensim.faults.injected").build();
    faults_counter.add(
        total_faults as u64,
        &[KeyValue::new("seed", seed.to_string())],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use heisensim_props::PropertyVerdict;

    #[test]
    fn test_emit_metrics_no_panic() {
        let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder().build();
        let verdicts = vec![
            PropertyVerdict {
                passed: true,
                property_name: "prop1".to_string(),
                expected: "ok".to_string(),
                actual: "ok".to_string(),
                details: vec![],
            },
            PropertyVerdict {
                passed: false,
                property_name: "prop2".to_string(),
                expected: "ok".to_string(),
                actual: "fail".to_string(),
                details: vec![],
            },
        ];
        emit_verdict_metrics(&provider, &verdicts, 42, 1.23, 5);
    }

    #[test]
    fn test_emit_metrics_empty_verdicts() {
        let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder().build();
        emit_verdict_metrics(&provider, &[], 42, 1.23, 0);
    }
}
