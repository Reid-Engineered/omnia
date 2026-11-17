//! CPU-only B7/WB5 workload over T2.4 sheets. Setup and warmup are outside timing.

use crate::{
    generate::{StressSheet, generate},
    stats::Summary,
};
use omnia_doc::{Doc, GRID_NM, Pt, SHEET_LIMIT_NM, Snap, hit_test, snap};

use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn parse_sheet(sheet: &str, samples: u32) -> Result<StressSheet, String> {
    if samples == 0 {
        return Err("samples must be positive".into());
    }
    match sheet {
        "100" => Ok(StressSheet::Parts100),
        "1k" => Ok(StressSheet::Parts1k),
        "10k" => Ok(StressSheet::Parts10k),
        "dense" => Ok(StressSheet::DenseWiring),
        "text" => Ok(StressSheet::TextHeavy),
        _ => Err(format!("unknown stress sheet `{sheet}`")),
    }
}

fn clamp_pt(p: Pt) -> Pt {
    Pt {
        x: p.x.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
        y: p.y.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
    }
}

pub fn run(sheet: &str, samples: u32, seed: u64) -> Result<Value, String> {
    let workload = parse_sheet(sheet, samples)?;
    let doc = generate(workload, seed);
    // At K3's 100% scale (20 px / GRID_NM), K14's 4 px is GRID_NM / 5.
    let tolerance = GRID_NM / 5;
    // Sweep every part and wire with exact and offset probes. Offsets may hit
    // neighboring items: about 93% hit on 10k, a conservative dense workload.
    // Deterministic coordinates are prepared before measuring queries.
    let mut probes = Vec::new();
    for (_, part) in doc.parts() {
        probes.extend([
            part.at,
            clamp_pt(
                part.at
                    + Pt {
                        x: tolerance,
                        y: -tolerance,
                    },
            ),
            clamp_pt(
                part.at
                    + Pt {
                        x: 3 * GRID_NM,
                        y: 3 * GRID_NM,
                    },
            ),
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
        black_box(hit_test(&doc, at, tolerance, &mut scratch));
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
            black_box(at),
            tolerance,
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

fn snap_probes(doc: &Doc, radius: i32) -> Vec<Pt> {
    let mut probes = Vec::new();
    for (id, part) in doc.parts() {
        if let Some(pins) = doc.pins(id) {
            for &pin in pins {
                probes.push(clamp_pt(
                    pin + Pt {
                        x: radius / 3,
                        y: -radius / 3,
                    },
                ));
            }
        }
        // Empty space between parts (generator pitch is 6 * GRID_NM) falls back to Grid.
        probes.push(clamp_pt(
            part.at
                + Pt {
                    x: 3 * GRID_NM + 200_000,
                    y: 3 * GRID_NM - 300_000,
                },
        ));
    }
    for (_, wire) in doc.wires() {
        let mid = Pt {
            x: ((i64::from(wire.a.x) + i64::from(wire.b.x)) / 2) as i32,
            y: ((i64::from(wire.a.y) + i64::from(wire.b.y)) / 2) as i32,
        };
        probes.extend([
            clamp_pt(
                wire.b
                    + Pt {
                        x: radius / 4,
                        y: radius / 4,
                    },
            ),
            clamp_pt(
                mid + Pt {
                    x: 0,
                    y: radius / 4,
                },
            ),
        ]);
    }
    probes
}

pub fn run_snap(sheet: &str, samples: u32, seed: u64) -> Result<Value, String> {
    let workload = parse_sheet(sheet, samples)?;
    let doc = generate(workload, seed);
    // At K3's 100% scale (20 px / GRID_NM), WK4/K14's 10 px snap radius is GRID_NM / 2.
    let radius = GRID_NM / 2;
    let probes = snap_probes(&doc, radius);
    let mut scratch = Vec::new();
    for &at in &probes {
        black_box(snap(&doc, at, GRID_NM, radius, &mut scratch));
    }
    let mut times = Vec::with_capacity(samples as usize);
    let mut pins = 0u32;
    let mut wire_ends = 0u32;
    let mut junctions = 0u32;
    let mut wire_bodies = 0u32;
    let mut grids = 0u32;
    for i in 0..samples {
        let at = probes[(i as usize * 7919) % probes.len()];
        let start = Instant::now();
        let result = black_box(snap(
            black_box(&doc),
            black_box(at),
            GRID_NM,
            radius,
            &mut scratch,
        ));
        let elapsed = start.elapsed();
        match result {
            Snap::Pin { .. } => pins += 1,
            Snap::WireEnd { .. } => wire_ends += 1,
            Snap::Junction { .. } => junctions += 1,
            Snap::WireBody { .. } => wire_bodies += 1,
            Snap::Grid(_) => grids += 1,
        }
        times.push(elapsed.as_secs_f64() * 1000.0);
    }
    let summary = Summary::from_samples(&mut times).unwrap();
    Ok(json!({
        "schema": crate::compare::SCHEMA, "kind": "hit",
        "config": { "sheet": sheet, "seed": seed, "samples": samples, "snap": true, "radius_nm": radius, "grid_nm": GRID_NM },
        "environment": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "window": null },
        "classes": { "pin": pins, "wire_end": wire_ends, "junction": junctions, "wire_body": wire_bodies, "grid": grids },
        "parts": doc.parts().count(), "wires": doc.wires().count(),
        "metrics": { "snap_ms": summary.to_json() },
        "reference_machine": false,
        "note": "WB5 requires the Q4 reference machine; this report does not certify the gate"
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

    #[test]
    fn snap_report_exercises_targets_and_comparison_schema() {
        let report = run_snap("100", 200, 42).unwrap();
        assert_eq!(report["parts"], 100);
        assert_eq!(report["wires"], 200);
        assert_eq!(report["config"]["snap"], true);
        assert_eq!(report["config"]["samples"], 200);
        assert!(report["metrics"]["snap_ms"]["p50"].is_number());
        assert!(report["metrics"]["snap_ms"]["p99"].is_number());
        let classes = &report["classes"];
        let total = classes["pin"].as_u64().unwrap()
            + classes["wire_end"].as_u64().unwrap()
            + classes["junction"].as_u64().unwrap()
            + classes["wire_body"].as_u64().unwrap()
            + classes["grid"].as_u64().unwrap();
        assert_eq!(total, 200);
        assert!(classes["pin"].as_u64().unwrap() > 0);
        assert!(classes["grid"].as_u64().unwrap() > 0);
        assert!(
            crate::compare::compare(&report, &report, 10.0, &[])
                .unwrap()
                .passed()
        );
        assert!(run_snap("bad", 100, 42).is_err());
        assert!(run_snap("10k", 0, 42).is_err());
    }
}
