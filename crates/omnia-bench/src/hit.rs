//! CPU-only B7 workload over T2.4 sheets. Setup and warmup are outside timing.

use crate::{
    generate::{StressSheet, generate},
    stats::Summary,
};
use omnia_doc::{GRID_NM, Pt, build_hit_index, hit_test};
use omnia_symbols::SymbolLibrary;
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

pub fn run(sheet: &str, samples: u32, seed: u64) -> Result<Value, String> {
    let workload = match sheet {
        "100" => StressSheet::Parts100,
        "1k" => StressSheet::Parts1k,
        "10k" => StressSheet::Parts10k,
        "dense" => StressSheet::DenseWiring,
        "text" => StressSheet::TextHeavy,
        _ => return Err(format!("unknown stress sheet `{sheet}`")),
    };
    if samples == 0 {
        return Err("samples must be positive".into());
    }
    let doc = generate(workload, seed);
    let symbols = SymbolLibrary::builtin();
    let bounds = |id| symbols.get(id).bounds;
    let index = build_hit_index(&doc, bounds).map_err(|e| e.to_string())?;
    // At K3's 100% scale (20 px / GRID_NM), K14's 4 px is GRID_NM / 5.
    let tolerance = GRID_NM / 5;
    // Sweep every part and wire with exact and offset probes. Offsets may hit
    // neighboring items: about 93% hit on 10k, a conservative dense workload.
    // Deterministic coordinates are prepared before measuring queries.
    let mut probes = Vec::new();
    for (_, part) in doc.parts() {
        probes.extend([
            part.at,
            part.at
                + Pt {
                    x: tolerance,
                    y: -tolerance,
                },
            part.at
                + Pt {
                    x: 3 * GRID_NM,
                    y: 3 * GRID_NM,
                },
        ]);
    }
    for (_, wire) in doc.wires() {
        probes.extend([
            wire.a,
            Pt {
                x: ((i64::from(wire.a.x) + i64::from(wire.b.x)) / 2) as i32,
                y: ((i64::from(wire.a.y) + i64::from(wire.b.y)) / 2) as i32,
            },
        ]);
    }
    let mut scratch = Vec::new();
    for &at in &probes {
        black_box(hit_test(&doc, &index, at, tolerance, bounds, &mut scratch));
    }
    let mut times = Vec::with_capacity(samples as usize);
    let mut hits = 0u32;
    for i in 0..samples {
        // A stride coprime to the 10k workload's 70k probes spreads short runs
        // over the whole sheet rather than timing only the first few rows.
        let at = probes[(i as usize * 7919) % probes.len()];
        let start = Instant::now();
        let hit = black_box(hit_test(
            black_box(&doc),
            black_box(&index),
            black_box(at),
            tolerance,
            bounds,
            &mut scratch,
        ));
        let elapsed = start.elapsed();
        hits += u32::from(hit.is_some());
        times.push(elapsed.as_secs_f64() * 1000.0);
    }
    let summary = Summary::from_samples(&mut times).unwrap();
    Ok(json!({
        "schema": crate::compare::SCHEMA, "kind": "hit",
        "config": { "sheet": sheet, "seed": seed, "samples": samples, "tolerance_nm": tolerance, "grid_nm": GRID_NM },
        "environment": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "window": null },
        "hits": hits,
        "parts": doc.parts().count(), "wires": doc.wires().count(),
        "metrics": { "hit_ms": summary.to_json() },
        "reference_machine": false,
        "note": "B7 requires the Q4 reference machine; this report does not certify the gate"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_uses_stress_sheet_and_comparison_schema() {
        let report = run("100", 100, 42).unwrap();
        assert_eq!(report["parts"], 100);
        assert_eq!(report["wires"], 200);
        assert_eq!(report["config"]["samples"], 100);
        assert!(report["metrics"]["hit_ms"]["p99"].is_number());
        assert!(
            crate::compare::compare(&report, &report, 10.0, &[])
                .unwrap()
                .passed()
        );
        assert!(run("bad", 100, 42).is_err());
        assert!(run("10k", 0, 42).is_err());
    }
}
