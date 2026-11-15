//! `bench compare base.json new.json --max-regression PCT`.
//!
//! Every number under a report's `metrics` object is a cost: lower is better. A metric
//! regresses when `new > base + |base| * PCT / 100`. A zero base therefore regresses on any
//! increase (B8 frames and B10 allocations are zero budgets). A metric in `base` that is
//! missing from `new` fails the comparison. Reports must have the same `schema`, `kind` and
//! `config`; `environment` may differ and only produces a warning. Every `--metrics` prefix
//! must select at least one metric of the base report, so a mistyped or absent prefix cannot
//! pass by comparing nothing.

use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SCHEMA: u64 = 1;

#[derive(Debug, PartialEq)]
pub struct Change {
    pub metric: String,
    pub base: f64,
    pub new: f64,
}

impl Change {
    /// Percent change from base; infinite when the base is zero.
    pub fn percent(&self) -> f64 {
        if self.base == 0.0 {
            if self.new == 0.0 { 0.0 } else { f64::INFINITY }
        } else {
            (self.new - self.base) / self.base.abs() * 100.0
        }
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub compared: Vec<Change>,
    pub regressions: Vec<Change>,
    /// Metrics in the base report but not the new one, with their base values.
    pub missing: Vec<(String, f64)>,
    pub warnings: Vec<String>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.regressions.is_empty() && self.missing.is_empty()
    }

    /// The comparison as JSON: one entry per metric, sorted by name, and the verdict.
    /// `change_percent` is null when the base is zero and the new value is not.
    pub fn to_json(&self, max_regression_percent: f64, only: &[String]) -> Value {
        let mut results: Vec<(String, Value)> = Vec::new();
        let mut push = |change: &Change, pass: bool| {
            let percent = change.percent();
            let direction = if change.new < change.base {
                "better"
            } else if change.new > change.base {
                "worse"
            } else {
                "same"
            };
            results.push((
                change.metric.clone(),
                json!({
                    "metric": change.metric,
                    "baseline": change.base,
                    "candidate": change.new,
                    "change_percent": percent.is_finite().then_some(percent),
                    "direction": direction,
                    "pass": pass,
                }),
            ));
        };
        for change in &self.compared {
            push(change, true);
        }
        for change in &self.regressions {
            push(change, false);
        }
        for (metric, base) in &self.missing {
            results.push((
                metric.clone(),
                json!({
                    "metric": metric,
                    "baseline": base,
                    "candidate": null,
                    "change_percent": null,
                    "direction": "missing",
                    "pass": false,
                }),
            ));
        }
        results.sort_by(|a, b| a.0.cmp(&b.0));
        json!({
            "schema": SCHEMA,
            "kind": "compare",
            "max_regression_percent": max_regression_percent,
            "metrics_filter": only,
            "verdict": if self.passed() { "pass" } else { "fail" },
            "counts": {
                "compared": self.compared.len() + self.regressions.len() + self.missing.len(),
                "regressions": self.regressions.len(),
                "missing": self.missing.len(),
            },
            "warnings": self.warnings,
            "results": results.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        })
    }
}

/// Compares two reports. `only` restricts the metrics to those whose dotted path starts with
/// one of the prefixes; empty means all. `Err` means the reports cannot be compared.
pub fn compare(
    base: &Value,
    new: &Value,
    max_regression_percent: f64,
    only: &[String],
) -> Result<Outcome, String> {
    if !max_regression_percent.is_finite() || max_regression_percent < 0.0 {
        return Err("--max-regression must be a non-negative number".into());
    }
    for field in ["schema", "kind", "config"] {
        if base.get(field).is_none() {
            return Err(format!("base report has no `{field}`"));
        }
        if base.get(field) != new.get(field) {
            return Err(format!(
                "reports differ in `{field}`: {} vs {}",
                base.get(field).unwrap_or(&Value::Null),
                new.get(field).unwrap_or(&Value::Null)
            ));
        }
    }
    if base["schema"].as_u64() != Some(SCHEMA) {
        return Err(format!("unsupported report schema {}", base["schema"]));
    }

    let base_metrics = metrics(base, "base")?;
    let new_metrics = metrics(new, "new")?;
    for prefix in only {
        let matches = |m: &BTreeMap<String, f64>| m.keys().any(|k| k.starts_with(prefix.as_str()));
        if !matches(&base_metrics) {
            return Err(if matches(&new_metrics) {
                format!(
                    "--metrics prefix `{prefix}` matches metrics only in the new report;                      nothing to compare"
                )
            } else {
                format!("--metrics prefix `{prefix}` matches no metric in either report")
            });
        }
    }
    let selected = |key: &str| only.is_empty() || only.iter().any(|p| key.starts_with(p.as_str()));

    let mut outcome = Outcome::default();
    if base.get("environment") != new.get("environment") {
        outcome.warnings.push(
            "environments differ (machine, display or scale); timing comparisons may not mean much"
                .into(),
        );
    }
    for (key, &base_value) in base_metrics.iter().filter(|(k, _)| selected(k)) {
        let Some(&new_value) = new_metrics.get(key) else {
            outcome.missing.push((key.clone(), base_value));
            continue;
        };
        let change = Change {
            metric: key.clone(),
            base: base_value,
            new: new_value,
        };
        let limit = base_value + base_value.abs() * max_regression_percent / 100.0;
        if new_value > limit {
            outcome.regressions.push(change);
        } else {
            outcome.compared.push(change);
        }
    }
    if outcome.compared.is_empty() && outcome.regressions.is_empty() && outcome.missing.is_empty() {
        return Err("no metrics selected for comparison".into());
    }
    for key in new_metrics.keys().filter(|k| selected(k)) {
        if !base_metrics.contains_key(key) {
            outcome
                .warnings
                .push(format!("{key}: only in the new report, not compared"));
        }
    }
    Ok(outcome)
}

/// Flattens `report.metrics` into dotted paths. Every leaf must be a finite number.
fn metrics(report: &Value, which: &str) -> Result<BTreeMap<String, f64>, String> {
    let Some(root) = report.get("metrics").filter(|m| m.is_object()) else {
        return Err(format!("{which} report has no `metrics` object"));
    };
    let mut out = BTreeMap::new();
    flatten(root, String::new(), &mut out).map_err(|e| format!("{which} report: {e}"))?;
    Ok(out)
}

fn flatten(value: &Value, path: String, out: &mut BTreeMap<String, f64>) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten(child, child_path, out)?;
            }
            Ok(())
        }
        Value::Number(n) => match n.as_f64().filter(|v| v.is_finite()) {
            Some(v) => {
                out.insert(path, v);
                Ok(())
            }
            None => Err(format!("metric {path} is not a finite number")),
        },
        other => Err(format!("metric {path} is not a number: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(frame_p99: f64, allocs_ours_max: f64, quads: f64) -> Value {
        json!({
            "schema": SCHEMA,
            "kind": "run",
            "config": { "sheet": "placeholder", "frames": 600 },
            "environment": { "scale_factor": 1.0 },
            "metrics": {
                "frame_ms": { "p50": 1.0, "p99": frame_p99 },
                "allocs_per_frame": { "ours": { "max": allocs_ours_max } },
                "primitives": { "quads": quads },
            },
        })
    }

    #[test]
    fn identical_reports_pass() {
        let a = report(2.0, 0.0, 1000.0);
        let outcome = compare(&a, &a, 10.0, &[]).unwrap();
        assert!(outcome.passed());
        assert_eq!(outcome.compared.len(), 4);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn regression_over_limit_fails_and_at_limit_passes() {
        let base = report(2.0, 0.0, 1000.0);
        assert!(
            compare(&base, &report(2.2, 0.0, 1100.0), 10.0, &[])
                .unwrap()
                .passed()
        );
        let outcome = compare(&base, &report(2.0, 0.0, 1110.0), 10.0, &[]).unwrap();
        assert!(!outcome.passed());
        assert_eq!(outcome.regressions[0].metric, "primitives.quads");
        assert!((outcome.regressions[0].percent() - 11.0).abs() < 1e-9);
    }

    #[test]
    fn zero_base_regresses_on_any_increase() {
        let outcome = compare(&report(2.0, 0.0, 10.0), &report(2.0, 1.0, 10.0), 10.0, &[]).unwrap();
        assert_eq!(outcome.regressions.len(), 1);
        assert_eq!(outcome.regressions[0].metric, "allocs_per_frame.ours.max");
        assert!(outcome.regressions[0].percent().is_infinite());
    }

    #[test]
    fn improvements_pass() {
        assert!(
            compare(&report(2.0, 1.0, 10.0), &report(1.0, 0.0, 5.0), 0.0, &[])
                .unwrap()
                .passed()
        );
    }

    #[test]
    fn only_filters_metrics() {
        let base = report(2.0, 0.0, 10.0);
        let slower = report(9.0, 0.0, 10.0);
        let only = [
            "primitives.".to_string(),
            "allocs_per_frame.ours".to_string(),
        ];
        let outcome = compare(&base, &slower, 10.0, &only).unwrap();
        assert!(outcome.passed());
        assert_eq!(outcome.compared.len(), 2);
        assert!(compare(&base, &slower, 10.0, &["nonexistent".into()]).is_err());
    }

    #[test]
    fn every_metrics_prefix_must_match() {
        let base = report(2.0, 0.0, 10.0);
        let mut no_primitives = base.clone();
        no_primitives["metrics"]
            .as_object_mut()
            .unwrap()
            .remove("primitives");
        // One prefix matches, the other matches nothing in either report: an error naming it.
        let only = [
            "allocs_per_frame.ours".to_string(),
            "primitives".to_string(),
        ];
        let error = compare(&no_primitives, &no_primitives, 10.0, &only).unwrap_err();
        assert!(error.contains("`primitives`"), "{error}");
        // A prefix only the new report has compares nothing either.
        let error = compare(&no_primitives, &base, 10.0, &only).unwrap_err();
        assert!(error.contains("`primitives`"), "{error}");
        // Both prefixes match: the comparison runs.
        assert!(compare(&base, &base, 10.0, &only).unwrap().passed());
    }

    #[test]
    fn json_lists_every_metric_and_the_verdict() {
        let base = report(2.0, 0.0, 10.0);
        let mut new = report(1.0, 1.0, 10.0);
        new["metrics"]["frame_ms"]
            .as_object_mut()
            .unwrap()
            .remove("p50");
        let outcome = compare(&base, &new, 10.0, &[]).unwrap();
        let json = outcome.to_json(10.0, &[]);
        assert_eq!(json["kind"], "compare");
        assert_eq!(json["verdict"], "fail");
        assert_eq!(json["counts"]["compared"], 4);
        assert_eq!(json["counts"]["regressions"], 1);
        assert_eq!(json["counts"]["missing"], 1);
        let results = json["results"].as_array().unwrap();
        let names: Vec<_> = results
            .iter()
            .map(|r| r["metric"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "allocs_per_frame.ours.max",
                "frame_ms.p50",
                "frame_ms.p99",
                "primitives.quads"
            ]
        );
        assert_eq!(
            results[0],
            json!({ "metric": "allocs_per_frame.ours.max", "baseline": 0.0, "candidate": 1.0,
                    "change_percent": null, "direction": "worse", "pass": false })
        );
        assert_eq!(
            results[1],
            json!({ "metric": "frame_ms.p50", "baseline": 1.0, "candidate": null,
                    "change_percent": null, "direction": "missing", "pass": false })
        );
        assert_eq!(
            results[2],
            json!({ "metric": "frame_ms.p99", "baseline": 2.0, "candidate": 1.0,
                    "change_percent": -50.0, "direction": "better", "pass": true })
        );
        assert_eq!(results[3]["direction"], "same");
        assert_eq!(results[3]["pass"], true);

        let passing = compare(&base, &base, 10.0, &[]).unwrap().to_json(10.0, &[]);
        assert_eq!(passing["verdict"], "pass");
    }

    #[test]
    fn missing_metric_fails() {
        let base = report(2.0, 0.0, 10.0);
        let mut new = base.clone();
        new["metrics"]["primitives"]
            .as_object_mut()
            .unwrap()
            .remove("quads");
        let outcome = compare(&base, &new, 10.0, &[]).unwrap();
        assert_eq!(outcome.missing, [("primitives.quads".to_string(), 10.0)]);
        assert!(!outcome.passed());
    }

    #[test]
    fn incomparable_reports_are_errors() {
        let base = report(2.0, 0.0, 10.0);
        let mut other = base.clone();
        other["config"]["frames"] = json!(300);
        assert!(compare(&base, &other, 10.0, &[]).is_err());
        let mut other = base.clone();
        other["kind"] = json!("idle");
        assert!(compare(&base, &other, 10.0, &[]).is_err());
        let mut other = base.clone();
        other["metrics"]["frame_ms"]["p50"] = json!("fast");
        assert!(compare(&base, &other, 10.0, &[]).is_err());
        assert!(compare(&base, &base, -1.0, &[]).is_err());
    }

    #[test]
    fn environment_difference_warns() {
        let base = report(2.0, 0.0, 10.0);
        let mut other = base.clone();
        other["environment"]["scale_factor"] = json!(1.5);
        let outcome = compare(&base, &other, 10.0, &[]).unwrap();
        assert!(outcome.passed());
        assert_eq!(outcome.warnings.len(), 1);
    }
}
