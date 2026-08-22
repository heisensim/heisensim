use crate::{DiffArgs, OutputFormat, parse_duration, parse_seed, resolve_faults};
use anyhow::Result;
use heisensim_core::types::VirtualTime;
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::collections::{BTreeMap, BTreeSet};

pub async fn handle_diff(args: DiffArgs) -> Result<i32> {
    let seed_a = parse_seed(&args.seed_a)?;
    let seed_b = parse_seed(&args.seed_b)?;

    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;
    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());

    let property_defs = if let Some(ref config_path) = args.config {
        crate::properties::load_and_validate(config_path)?
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

    if args.output == OutputFormat::Json {
        let out = serde_json::json!({
            "seed_a": format!("0x{:04X}", seed_a),
            "seed_b": format!("0x{:04X}", seed_b),
            "hash_a": format!("{:016x}", res_a.hash),
            "hash_b": format!("{:016x}", res_b.hash),
            "faults_a": res_a.total_faults,
            "faults_b": res_b.total_faults,
            "failures_a": res_a.total_failures,
            "failures_b": res_b.total_failures,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(0);
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
        ha[..8].to_string() + "...",
        hb[..8].to_string() + "..."
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
        // Collect faults and probes
        let mut f_a = Vec::new();
        let mut p_a = std::collections::BTreeMap::new();
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

        let mut f_b = Vec::new();
        let mut p_b = std::collections::BTreeMap::new();
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

        // Match faults
        let mut fa_idx = 0;
        let mut fb_idx = 0;
        while fa_idx < f_a.len() || fb_idx < f_b.len() {
            let sa = if fa_idx < f_a.len() { &f_a[fa_idx] } else { "" };
            let sb = if fb_idx < f_b.len() { &f_b[fb_idx] } else { "" };
            if sa != sb {
                println!("║  t={:<3}s  {:<20} │  {:<26} ║", t.as_secs(), sa, sb);
            }
            fa_idx += 1;
            fb_idx += 1;
        }

        // Match probes
        let mut all_probes: std::collections::BTreeSet<String> = BTreeSet::new();
        all_probes.extend(p_a.keys().cloned());
        all_probes.extend(p_b.keys().cloned());

        for p in all_probes {
            let a_succ = p_a.get(&p).copied();
            let b_succ = p_b.get(&p).copied();
            if a_succ.is_some() && b_succ.is_some() && a_succ != b_succ {
                let sa = if a_succ.unwrap() { "pass" } else { "fail" };
                let sb = if b_succ.unwrap() { "pass" } else { "fail" };
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

    if !res_a.verdicts.is_empty() {
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  PROPERTY COMPARISON                                         ║");
        println!("╠══════════════════════════════════════════════════════════════╣");

        for i in 0..res_a.verdicts.len() {
            let va = &res_a.verdicts[i];
            let vb = &res_b.verdicts[i];
            let ia = if va.passed { "✅" } else { "❌" };
            let ib = if vb.passed { "✅" } else { "❌" };
            let aa = va.actual.to_string();
            let ab = vb.actual.to_string();
            let n_aa = if aa.len() > 12 {
                aa[..12].to_string()
            } else {
                aa
            };
            let n_ab = if ab.len() > 24 {
                ab[..24].to_string()
            } else {
                ab
            };
            println!(
                "║  {:<14} {} {:<12} │  {} {:<24} ║",
                va.property_name, ia, n_aa, ib, n_ab
            );
        }
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(0)
}
