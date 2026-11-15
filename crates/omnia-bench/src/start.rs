//! `bench start`: process start to first presented frame (B1, B2).
//!
//! Each run spawns this executable as `start-child`, which opens the window and prints
//! [`READY_LINE`] once its first frame has been drawn and presented. The time is measured
//! in the parent from just before `spawn` to reading that line, so it includes process
//! creation, loading, GPUI and DirectX start-up and the first frame.

use crate::compare::SCHEMA;
use crate::harness::{READY_LINE, T01_WINDOW};
use crate::scene::Sheet;
use crate::stats::round;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CHILD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
pub struct StartConfig {
    pub sheet: Sheet,
    pub runs: u32,
}

pub fn start(config: StartConfig) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut samples_ms = Vec::with_capacity(config.runs as usize);
    for run in 0..config.runs {
        let begin = Instant::now();
        let mut child = Command::new(&exe)
            .args(["start-child", "--sheet", config.sheet.name()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn start-child: {e}"))?;
        let stdout = child.stdout.take().ok_or("child has no stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.trim() == READY_LINE {
                    let _ = tx.send(Instant::now());
                }
            }
        });
        let ready = match rx.recv_timeout(CHILD_TIMEOUT) {
            Ok(at) => at,
            Err(_) => {
                let _ = child.kill();
                return Err(format!(
                    "run {run}: no first frame within {CHILD_TIMEOUT:?}"
                ));
            }
        };
        samples_ms.push(ready.duration_since(begin).as_secs_f64() * 1000.0);
        wait_for_exit(&mut child, run)?;
    }

    let mut sorted = samples_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let median = median(&sorted).ok_or("no runs")?;
    Ok(json!({
        "schema": SCHEMA,
        "kind": "start",
        "config": {
            "sheet": config.sheet.name(),
            "runs": config.runs,
            "window_logical": [T01_WINDOW.0, T01_WINDOW.1],
        },
        "environment": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map_or(0, |n| n.get()),
        },
        "samples_ms": samples_ms.iter().map(|&s| round(s)).collect::<Vec<_>>(),
        "metrics": {
            "start_ms": {
                "median": round(median),
                "max": round(*sorted.last().unwrap_or(&median)),
            },
        },
    }))
}

fn wait_for_exit(child: &mut Child, run: u32) -> Result<(), String> {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        match child.try_wait().map_err(|e| format!("wait: {e}"))? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("run {run}: start-child exited with {status}")),
            None if Instant::now() > deadline => {
                let _ = child.kill();
                return Err(format!("run {run}: start-child did not quit"));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Median of sorted samples; the mean of the middle two for an even count.
pub fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn median() {
        assert_eq!(super::median(&[]), None);
        assert_eq!(super::median(&[1.0, 2.0, 9.0]), Some(2.0));
        assert_eq!(super::median(&[1.0, 2.0, 4.0, 9.0]), Some(3.0));
    }
}
