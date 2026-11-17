//! CPU-only B7/WB5 workload over T2.4 sheets. Setup and warmup are outside timing.

use crate::{
    generate::{StressSheet, generate},
    stats::Summary,
};
use omnia_doc::{Doc, GRID_NM, Pt, SHEET_LIMIT_NM, Snap, hit_test, snap};

use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    hint::black_box,
    time::Instant,
};

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

/// WD5 class names in rank order, as reported in the JSON.
const CLASSES: [&str; 5] = ["pin", "wire_end", "junction", "wire_body", "grid"];

fn class_index(s: Snap) -> usize {
    match s {
        Snap::Pin { .. } => 0,
        Snap::WireEnd { .. } => 1,
        Snap::Junction { .. } => 2,
        Snap::WireBody { .. } => 3,
        Snap::Grid(_) => 4,
    }
}

fn class_counts(counts: [u32; 5]) -> Value {
    let mut map = serde_json::Map::new();
    for (name, n) in CLASSES.into_iter().zip(counts) {
        map.insert(name.into(), n.into());
    }
    Value::Object(map)
}

/// Classes that no cursor position can produce on `doc` with this `grid`, from the
/// sheet's structure alone (independent of the probes):
/// - `junction` needs a point where three or more wire ends meet and no pin lies
///   (a pin there is the same distance away and ranks higher);
/// - `wire_body` needs a grid point strictly inside a wire (a body target at an end
///   is beaten by that end, which is then in the radius too).
fn unreachable_classes(doc: &Doc, grid: i32) -> Vec<&'static str> {
    let mut ends: HashMap<Pt, u32> = HashMap::new();
    let mut interior_grid_point = false;
    for (_, w) in doc.wires() {
        *ends.entry(w.a).or_default() += 1;
        *ends.entry(w.b).or_default() += 1;
        let (lo, hi) = if w.a.y == w.b.y {
            (w.a.x.min(w.b.x), w.a.x.max(w.b.x))
        } else {
            (w.a.y.min(w.b.y), w.a.y.max(w.b.y))
        };
        // Smallest multiple of `grid` strictly greater than `lo`.
        let next = (i64::from(lo).div_euclid(i64::from(grid)) + 1) * i64::from(grid);
        interior_grid_point |= next < i64::from(hi);
    }
    let pins: HashSet<Pt> = doc
        .parts()
        .flat_map(|(id, _)| doc.pins(id).into_iter().flatten().copied())
        .collect();
    let junction_point = ends.iter().any(|(p, &n)| n >= 3 && !pins.contains(p));
    let mut out = Vec::new();
    if !junction_point {
        out.push("junction");
    }
    if !interior_grid_point {
        out.push("wire_body");
    }
    out
}

/// Deterministic snap probes, in item id order. Each kind aims at one WD5 class;
/// the reported counts come from the actual `snap` results, not from these aims.
fn snap_probes(doc: &Doc, radius: i32) -> Vec<Pt> {
    let mut probes = Vec::new();
    for (id, part) in doc.parts() {
        // Pin: a third of the radius off each pin, diagonally.
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
        // Grid: empty space between parts (generator pitch is 6 * GRID_NM).
        probes.push(clamp_pt(
            part.at
                + Pt {
                    x: 3 * GRID_NM + 200_000,
                    y: 3 * GRID_NM - 300_000,
                },
        ));
    }
    for (_, wire) in doc.wires() {
        // Unit direction from `a` to `b` and its perpendicular (wires are axis-aligned).
        let dir = Pt {
            x: (wire.b.x - wire.a.x).signum(),
            y: (wire.b.y - wire.a.y).signum(),
        };
        let perp = Pt {
            x: -dir.y,
            y: dir.x,
        };
        let scale = |v: Pt, k: i32| Pt {
            x: v.x * k,
            y: v.y * k,
        };
        let mid = Pt {
            x: ((i64::from(wire.a.x) + i64::from(wire.b.x)) / 2) as i32,
            y: ((i64::from(wire.a.y) + i64::from(wire.b.y)) / 2) as i32,
        };
        probes.extend([
            // Wire end: past each end, continuing the wire, half the radius out.
            // Where that end is a pin, or a pin is in the radius, the pin wins.
            clamp_pt(wire.b + scale(dir, radius / 2)),
            clamp_pt(wire.a - scale(dir, radius / 2)),
            // Wire end or junction: diagonal off the far end (T4.2's probe).
            clamp_pt(
                wire.b
                    + Pt {
                        x: radius / 4,
                        y: radius / 4,
                    },
            ),
            // Wire body: off the middle of the wire, a quarter of the radius aside.
            clamp_pt(mid + scale(perp, radius / 4)),
            // Wire body, away from the ends: nine tenths of the radius aside of the
            // middle. On a wire with an interior grid point this lands in the body
            // class; on a stub with none, every tier is scanned and the grid wins.
            clamp_pt(mid + scale(perp, radius * 9 / 10)),
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
    // Warmup over every probe, outside timing; it also records each probe's class.
    let mut probe_classes = [0u32; 5];
    for &at in &probes {
        probe_classes[class_index(black_box(snap(&doc, at, GRID_NM, radius, &mut scratch)))] += 1;
    }
    let mut times = Vec::with_capacity(samples as usize);
    let mut classes = [0u32; 5];
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
        classes[class_index(result)] += 1;
        times.push(elapsed.as_secs_f64() * 1000.0);
    }
    let summary = Summary::from_samples(&mut times).unwrap();
    Ok(json!({
        "schema": crate::compare::SCHEMA, "kind": "hit",
        "config": { "sheet": sheet, "seed": seed, "samples": samples, "snap": true, "radius_nm": radius, "grid_nm": GRID_NM },
        "environment": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "window": null },
        "classes": class_counts(classes),
        "probes": probes.len(),
        "probe_classes": class_counts(probe_classes),
        "unreachable_classes": unreachable_classes(&doc, GRID_NM),
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

    fn class_count(report: &Value, field: &str, class: &str) -> u64 {
        report[field][class].as_u64().unwrap()
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
        let total: u64 = CLASSES
            .iter()
            .map(|c| class_count(&report, "classes", c))
            .sum();
        assert_eq!(total, 200);
        let probes: u64 = CLASSES
            .iter()
            .map(|c| class_count(&report, "probe_classes", c))
            .sum();
        assert_eq!(probes, report["probes"].as_u64().unwrap());
        assert!(
            crate::compare::compare(&report, &report, 10.0, &[])
                .unwrap()
                .passed()
        );
        assert!(run_snap("bad", 100, 42).is_err());
        assert!(run_snap("10k", 0, 42).is_err());
    }

    /// WB5's gate sheet: every class the 10k sheet can produce is measured, both over
    /// the whole probe set and in the timed samples. A class is excused only when the
    /// sheet's structure rules it out (`unreachable_classes`); on T2.4's 10k sheet that
    /// is `junction` (only pins carry more than one wire end) and `wire_body` (every
    /// wire is a half-grid stub whose one grid point is an end).
    #[test]
    fn snap_report_on_10k_measures_every_reachable_class() {
        let report = run_snap("10k", 20_000, 42).unwrap();
        let unreachable: Vec<&str> = report["unreachable_classes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(unreachable, ["junction", "wire_body"]);
        for class in CLASSES {
            let all = class_count(&report, "probe_classes", class);
            let timed = class_count(&report, "classes", class);
            if unreachable.contains(&class) {
                assert_eq!((all, timed), (0, 0), "{class} is unreachable on 10k");
            } else {
                assert!(all > 0 && timed > 0, "{class}: {all} probes, {timed} timed");
            }
        }
    }

    /// `unreachable_classes` reports a class as soon as the sheet can produce it.
    #[test]
    fn unreachable_classes_tracks_sheet_structure() {
        use omnia_doc::{Label, Orient, Part, Rect, SymbolId, Wire};
        let wire = |a: Pt, b: Pt| Wire { a, b };
        let g = GRID_NM;
        let j = Pt { x: g, y: 0 };
        let part = (
            Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "".into(),
            },
            vec![Pt::ZERO],
            Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
        );
        let stubs = [
            wire(Pt::ZERO, Pt { x: g / 2, y: 0 }),
            wire(Pt::ZERO, Pt { x: 0, y: g / 2 }),
        ];
        let doc = Doc::from_items([part.clone()], stubs, Vec::<Label>::new()).unwrap();
        assert_eq!(unreachable_classes(&doc, g), ["junction", "wire_body"]);
        let star = [
            wire(Pt::ZERO, j),
            wire(j, Pt { x: g, y: 2 * g }),
            wire(j, Pt { x: g, y: -2 * g }),
        ];
        let doc = Doc::from_items([part], star, Vec::<Label>::new()).unwrap();
        assert!(unreachable_classes(&doc, g).is_empty());
    }
}
