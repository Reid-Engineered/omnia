//! `bench compare` through the binary: exit codes and `--json`.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn report(dir: &Path, name: &str, frame_p99: f64, primitives: bool) -> String {
    let mut metrics = json!({
        "frame_ms": { "p99": frame_p99 },
        "allocs_per_frame": { "ours": { "max": 0.0 } },
    });
    if primitives {
        metrics["primitives"] = json!({ "quads": 100.0 });
    }
    let report = json!({
        "schema": 1,
        "kind": "run",
        "config": { "sheet": "placeholder", "frames": 600 },
        "environment": {},
        "metrics": metrics,
    });
    let path = dir.join(name);
    std::fs::write(&path, report.to_string()).unwrap();
    path.to_str().unwrap().to_string()
}

fn bench(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_omnia-bench"))
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.code().expect("exited with a code"), stderr)
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn json_is_written_on_pass_and_on_regression() {
    let dir = dir("json_is_written_on_pass_and_on_regression");
    let base = report(&dir, "base.json", 2.0, true);
    let slow = report(&dir, "slow.json", 3.0, true);
    let out = dir.join("out.json");
    let out_arg = out.to_str().unwrap();

    let (code, stderr) = bench(&["compare", &base, &base, "--json", out_arg]);
    assert_eq!(code, 0, "{stderr}");
    let json = read(&out);
    assert_eq!(json["verdict"], "pass");
    assert_eq!(json["results"].as_array().unwrap().len(), 3);

    let (code, stderr) = bench(&[
        "compare",
        &base,
        &slow,
        "--max-regression",
        "10",
        "--json",
        out_arg,
    ]);
    assert_eq!(code, 1, "{stderr}");
    let json = read(&out);
    assert_eq!(json["verdict"], "fail");
    assert_eq!(json["max_regression_percent"], 10.0);
    let p99 = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["metric"] == "frame_ms.p99")
        .unwrap();
    assert_eq!(
        *p99,
        json!({ "metric": "frame_ms.p99", "baseline": 2.0, "candidate": 3.0,
                "change_percent": 50.0, "direction": "worse", "pass": false })
    );
}

#[test]
fn metrics_prefix_matching_nothing_is_a_usage_error() {
    let dir = dir("metrics_prefix_matching_nothing_is_a_usage_error");
    let base = report(&dir, "base.json", 2.0, false);
    let out = dir.join("out.json");
    let (code, stderr) = bench(&[
        "compare",
        &base,
        &base,
        "--metrics",
        "allocs_per_frame.ours,primitives",
        "--json",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("`primitives`"), "{stderr}");
    assert!(
        !out.exists(),
        "no verdict is written for an incomparable request"
    );

    let (code, stderr) = bench(&[
        "compare",
        &base,
        &base,
        "--metrics",
        "allocs_per_frame.ours",
    ]);
    assert_eq!(code, 0, "{stderr}");
}
