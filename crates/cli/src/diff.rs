use crate::{DiffArgs, OutputFormat, parse_duration, parse_seed, resolve_faults};
use anyhow::{Context, Result};
use heisensim_core::types::VirtualTime;
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::collections::{BTreeMap, BTreeSet};

pub async fn handle_diff(args: DiffArgs) -> Result<i32> {
    let seed_a = parse_seed(&args.seed_a)?;
    let seed_b = parse_seed(&args.seed_b)?;

    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;
    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());

    let property_defs: Vec<crate::properties::PropertyDef> =
        if let Some(ref config_path) = args.config {
            let config_str =
                std::fs::read_to_string(config_path).context("Failed to read config file")?;
            let config: crate::properties::PropertiesConfig =
                toml::from_str(&config_str).unwrap_or_default();
            crate::properties::resolve_with_template(
                args.property_template.as_ref(),
                &config.properties,
                config.template.as_ref(),
            )
        } else if let Some(ref tmpl) = args.property_template {
            tmpl.to_property_defs()
        } else {
            Vec::new()
        };

    let config_a = crate::dst::DstConfig {
        seed: seed_a,
        duration: VirtualTime::from_millis(duration.as_millis() as u64),
        warmup: VirtualTime::from_millis(warmup.as_millis() as u64),
        faults: resolved_faults.clone(),
        pod_count: 3,
        probe_interval: VirtualTime::from_secs(5),
        property_defs: property_defs.clone(),
    };

    let config_b = crate::dst::DstConfig {
        seed: seed_b,
        ..config_a.clone()
    };

    let res_a = crate::dst::run(config_a)?;
    let res_b = crate::dst::run(config_b)?;

    match args.output {
        OutputFormat::Json => {
            // Build fault timeline diff
            let fault_diff = build_fault_diff(&res_a.events, &res_b.events);
            // Build verdict comparison
            let verdict_diff: Vec<_> = build_verdict_diff(&res_a.verdicts, &res_b.verdicts);

            let out = serde_json::json!({
                "seed_a": seed_a,
                "seed_a_hex": format!("0x{:04X}", seed_a),
                "seed_b": seed_b,
                "seed_b_hex": format!("0x{:04X}", seed_b),
                "hash_a": format!("{:016x}", res_a.hash),
                "hash_b": format!("{:016x}", res_b.hash),
                "hashes_match": res_a.hash == res_b.hash,
                "faults_a": res_a.total_faults,
                "faults_b": res_b.total_faults,
                "failures_a": res_a.total_failures,
                "failures_b": res_b.total_failures,
                "fault_timeline_diff": fault_diff,
                "verdict_diff": verdict_diff,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(0);
        }
        OutputFormat::Junit | OutputFormat::Html => {
            anyhow::bail!(
                "diff only supports --output text or --output json (got {:?})",
                args.output
            );
        }
        OutputFormat::Text => {} // fall through to text rendering
    }

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  HEISENSIM DIFF                                              ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Seed A: 0x{:04X}          │  Seed B: 0x{:04X}                   ║",
        seed_a, seed_b
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                    Seed A      │  Seed B                     ║");
    let ha = format!("{:016x}", res_a.hash);
    let hb = format!("{:016x}", res_b.hash);
    println!(
        "║  Hash:         {:<15} │  {:<26} ║",
        truncate_str(&ha, 11).to_string() + "...",
        truncate_str(&hb, 11).to_string() + "..."
    );
    println!(
        "║  Faults:       {:<15} │  {:<26} ║",
        res_a.total_faults, res_b.total_faults
    );
    println!(
        "║  Failures:     {:<15} │  {:<26} ║",
        res_a.total_failures, res_b.total_failures
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  FAULT TIMELINE DIFF                                         ║");
    println!("╠══════════════════════════════════════════════════════════════╣");

    // Group events by elapsed time
    let mut times: BTreeMap<std::time::Duration, (Vec<&TimelineEvent>, Vec<&TimelineEvent>)> =
        BTreeMap::new();
    for e in &res_a.events {
        times.entry(e.elapsed).or_default().0.push(e);
    }
    for e in &res_b.events {
        times.entry(e.elapsed).or_default().1.push(e);
    }

    for (t, (evs_a, evs_b)) in times {
        // Collect faults by identity for comparison
        let mut f_a: Vec<String> = Vec::new();
        let mut p_a: BTreeMap<String, bool> = BTreeMap::new();
        for e in evs_a {
            match &e.kind {
                EventKind::FaultInjected {
                    fault_kind, target, ..
                } => {
                    f_a.push(format!("{} {}", fault_kind, target));
                }
                EventKind::ProbeSuccess { probe_name, .. } => {
                    p_a.insert(probe_name.clone(), true);
                }
                EventKind::ProbeFailed { probe_name, .. } => {
                    p_a.insert(probe_name.clone(), false);
                }
                _ => {}
            }
        }

        let mut f_b: Vec<String> = Vec::new();
        let mut p_b: BTreeMap<String, bool> = BTreeMap::new();
        for e in evs_b {
            match &e.kind {
                EventKind::FaultInjected {
                    fault_kind, target, ..
                } => {
                    f_b.push(format!("{} {}", fault_kind, target));
                }
                EventKind::ProbeSuccess { probe_name, .. } => {
                    p_b.insert(probe_name.clone(), true);
                }
                EventKind::ProbeFailed { probe_name, .. } => {
                    p_b.insert(probe_name.clone(), false);
                }
                _ => {}
            }
        }

        // Compare faults by identity using multiset diff
        let mut remaining_b: Vec<bool> = vec![true; f_b.len()];
        let mut only_a: Vec<String> = Vec::new();
        for fa in &f_a {
            let mut matched = false;
            for (i, fb) in f_b.iter().enumerate() {
                if remaining_b[i] && fa == fb {
                    remaining_b[i] = false;
                    matched = true;
                    break;
                }
            }
            if !matched {
                only_a.push(fa.clone());
            }
        }
        let only_b: Vec<String> = f_b
            .iter()
            .enumerate()
            .filter(|(i, _)| remaining_b[*i])
            .map(|(_, f)| f.clone())
            .collect();

        let max_diff = only_a.len().max(only_b.len());
        for i in 0..max_diff {
            let sa = only_a.get(i).map(|s| s.as_str()).unwrap_or("");
            let sb = only_b.get(i).map(|s| s.as_str()).unwrap_or("");
            println!("║  t={:<3}s  {:<20} │  {:<26} ║", t.as_secs(), sa, sb);
        }

        // Compare probes — show all, including those only in one run
        let mut all_probes: BTreeSet<String> = BTreeSet::new();
        all_probes.extend(p_a.keys().cloned());
        all_probes.extend(p_b.keys().cloned());

        for p in all_probes {
            let a_succ = p_a.get(&p).copied();
            let b_succ = p_b.get(&p).copied();
            if a_succ != b_succ {
                let sa = match a_succ {
                    Some(true) => "pass",
                    Some(false) => "fail",
                    None => "—",
                };
                let sb = match b_succ {
                    Some(true) => "pass",
                    Some(false) => "fail",
                    None => "—",
                };
                println!(
                    "║  t={:<3}s  {} {:<11} │  {} {:<17} ║",
                    t.as_secs(),
                    p,
                    sa,
                    p,
                    sb
                );
            }
        }
    }

    // Property verdict comparison — handle mismatched lengths safely
    let max_verdicts = res_a.verdicts.len().max(res_b.verdicts.len());
    if max_verdicts > 0 {
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  PROPERTY COMPARISON                                         ║");
        println!("╠══════════════════════════════════════════════════════════════╣");

        for i in 0..max_verdicts {
            let va = res_a.verdicts.get(i);
            let vb = res_b.verdicts.get(i);
            let name = va.or(vb).map(|v| v.property_name.as_str()).unwrap_or("?");
            let ia = va
                .map(|v| if v.passed { "✅" } else { "❌" })
                .unwrap_or("—");
            let ib = vb
                .map(|v| if v.passed { "✅" } else { "❌" })
                .unwrap_or("—");
            let aa = va.map(|v| v.actual.to_string()).unwrap_or_default();
            let ab = vb.map(|v| v.actual.to_string()).unwrap_or_default();
            println!(
                "║  {:<14} {} {:<12} │  {} {:<24} ║",
                truncate_str(name, 14),
                ia,
                truncate_str(&aa, 12),
                ib,
                truncate_str(&ab, 24)
            );
        }
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(0)
}

/// Unicode-safe string truncation.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Build fault timeline diff for JSON output.
fn build_fault_diff(
    events_a: &[TimelineEvent],
    events_b: &[TimelineEvent],
) -> Vec<serde_json::Value> {
    let mut times: BTreeMap<std::time::Duration, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for e in events_a {
        if let EventKind::FaultInjected {
            fault_kind, target, ..
        } = &e.kind
        {
            times
                .entry(e.elapsed)
                .or_default()
                .0
                .push(format!("{} {}", fault_kind, target));
        }
    }
    for e in events_b {
        if let EventKind::FaultInjected {
            fault_kind, target, ..
        } = &e.kind
        {
            times
                .entry(e.elapsed)
                .or_default()
                .1
                .push(format!("{} {}", fault_kind, target));
        }
    }

    let mut diffs = Vec::new();
    for (t, (mut fa, mut fb)) in times {
        fa.sort();
        fb.sort();
        if fa != fb {
            diffs.push(serde_json::json!({
                "time_secs": t.as_secs(),
                "seed_a": fa,
                "seed_b": fb,
            }));
        }
    }
    diffs
}

/// Build verdict comparison for JSON output.
fn build_verdict_diff(
    verdicts_a: &[heisensim_props::PropertyVerdict],
    verdicts_b: &[heisensim_props::PropertyVerdict],
) -> Vec<serde_json::Value> {
    let max = verdicts_a.len().max(verdicts_b.len());
    let mut diffs = Vec::new();
    for i in 0..max {
        let va = verdicts_a.get(i);
        let vb = verdicts_b.get(i);
        diffs.push(serde_json::json!({
            "property": va.or(vb).map(|v| &v.property_name),
            "seed_a_passed": va.map(|v| v.passed),
            "seed_a_actual": va.map(|v| v.actual.to_string()),
            "seed_b_passed": vb.map(|v| v.passed),
            "seed_b_actual": vb.map(|v| v.actual.to_string()),
        }));
    }
    diffs
}
