//! `bench`: the unattended benchmark harness. Run `bench help` for usage.

use omnia_bench::compare::compare;
use omnia_bench::driver::Script;
use omnia_bench::harness::{self, IdleConfig, RunConfig};
use omnia_bench::scene::Sheet;
use omnia_bench::spike::SpikeError;
use omnia_bench::start::{self, StartConfig};
use omnia_bench::{CountingAlloc, stats::round};
use serde_json::Value;
use std::process::ExitCode;
use std::time::Duration;

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

const USAGE: &str = "\
usage: bench <command> [options]

  run     [--sheet empty|100|1k|10k|dense|text|t01|placeholder] [--script pan|zoom]
          [--frames 600] [--warmup 60] [--size 1280x800] [--headless] [--json FILE]
  spike   s1..s6|s8 [--count N] [--mode fixed|sweep|square|rounded|sprite|direct|direct-sweep|cached|repaint|quality|monitors|displays]\n          [--frames 600] [--warmup 60] [--size 3072x1728] [--json FILE]\n  start   [--sheet empty|t01|placeholder] [--runs 10] [--size 960x640] [--json FILE]
  idle    [--sheet empty|t01|placeholder] [--seconds 10] [--settle-ms 1000] [--size 960x640] [--json FILE]
  hit     [--sheet 10k|1k|100|dense|text] [--samples 20000] [--seed 42] [--snap] [--json FILE]
  compare BASE.json NEW.json [--max-regression 10] [--metrics PREFIX[,PREFIX..]]
          [--json FILE]
  spike   s7 [--seconds 60] [--out FILE] | s7 --summarise FILE
  spike   chrome [--seconds 60] [--png FILE]
  spike   shell-cache [--hover-frames 300] [--pan-frames 600] [--warmup 60] [--size 1440x900] [--json FILE]

Every command exits by itself. Exit codes: 0 ok; 1 compare found a regression or a missing
metric; 2 usage or input error, including reports that cannot be compared or a --metrics
prefix that selects no base metric; 3 the run failed or timed out. compare --json writes
the per-metric result and the verdict whether it passes or finds a regression.";

/// Command-line options after the subcommand: `--name value` pairs, `--flag` switches, and positionals.
struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>, allowed: &[&str]) -> Result<Self, String> {
        Self::parse_with_flags(args, allowed, &[])
    }

    fn parse_with_flags(
        args: impl IntoIterator<Item = String>,
        allowed: &[&str],
        allowed_flags: &[&str],
    ) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut options = Vec::new();
        let mut flags = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if let Some(name) = arg.strip_prefix("--") {
                if allowed_flags.contains(&name) {
                    if flags.iter().any(|n| n == name) {
                        return Err(format!("--{name} given twice"));
                    }
                    flags.push(name.to_string());
                } else if allowed.contains(&name) {
                    if options.iter().any(|(n, _)| n == name) {
                        return Err(format!("--{name} given twice"));
                    }
                    let value = args.next().ok_or(format!("--{name} needs a value"))?;
                    options.push((name.to_string(), value));
                } else {
                    return Err(format!("unknown option --{name}"));
                }
            } else {
                positional.push(arg);
            }
        }
        Ok(Self {
            positional,
            options,
            flags,
        })
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|n| n == name)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        self.get(name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| format!("--{name}: `{v}` is not a valid number"))
        })
    }

    fn no_positionals(&self) -> Result<(), String> {
        match self.positional.first() {
            Some(extra) => Err(format!("unexpected argument `{extra}`")),
            None => Ok(()),
        }
    }
}

enum Failure {
    Usage(String),
    Run(String),
    Regression,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let result = match command.as_str() {
        "run" => cmd_run(args),
        "spike" => cmd_spike(args),
        "start" => cmd_start(args),
        "idle" => cmd_idle(args),
        "compare" => cmd_compare(args),
        "hit" => cmd_hit(args),
        "start-child" => cmd_start_child(args),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(Failure::Usage(format!("unknown command `{other}`"))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Regression) => ExitCode::from(1),
        Err(Failure::Usage(message)) => {
            eprintln!("bench: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Run(message)) => {
            eprintln!("bench: {message}");
            ExitCode::from(3)
        }
    }
}

fn usage<T>(result: Result<T, String>) -> Result<T, Failure> {
    result.map_err(Failure::Usage)
}

fn positive(name: &str, value: u32) -> Result<u32, Failure> {
    if value == 0 {
        return Err(Failure::Usage(format!("--{name} must be at least 1")));
    }
    Ok(value)
}

fn cmd_run(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse_with_flags(
        args,
        &["sheet", "script", "frames", "warmup", "size", "json"],
        &["headless"],
    ))?;
    usage(args.no_positionals())?;
    let size = usage(parse_size(args.get("size").unwrap_or("1280x800")))?;
    let config = RunConfig {
        sheet: usage(Sheet::parse(args.get("sheet").unwrap_or("placeholder")))?,
        script: usage(Script::parse(args.get("script").unwrap_or("pan")))?,
        frames: positive("frames", usage(args.number("frames", 600))?)?,
        // Warmup 1 is the minimum: the baseline is taken after the last warm-up frame.
        warmup: positive("warmup", usage(args.number("warmup", 60))?)?,
        size,
    };
    if config.script == Script::Zoom && !matches!(config.sheet, Sheet::Empty | Sheet::Document(_)) {
        return Err(Failure::Usage(
            "--script zoom needs --sheet empty or a generated sheet".into(),
        ));
    }
    if args.flag("headless") {
        #[cfg(feature = "headless")]
        {
            if !matches!(config.sheet, Sheet::Document(_)) {
                return Err(Failure::Usage("--headless needs a generated sheet".into()));
            }
            let report = omnia_bench::scripts::run_headless(config).map_err(Failure::Run)?;
            return finish(&report, args.get("json"));
        }
        #[cfg(not(feature = "headless"))]
        return Err(Failure::Usage(
            "--headless needs --features headless".into(),
        ));
    }
    let report = harness::run(config).map_err(Failure::Run)?;
    finish(&report, args.get("json"))
}

fn cmd_start(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse(args, &["sheet", "runs", "size", "json"]))?;
    usage(args.no_positionals())?;
    let config = StartConfig {
        size: usage(parse_size(args.get("size").unwrap_or("960x640")))?,
        sheet: usage(Sheet::parse(args.get("sheet").unwrap_or("empty")))?,
        runs: positive("runs", usage(args.number("runs", 10))?)?,
    };
    let report = start::start(config).map_err(Failure::Run)?;
    finish(&report, args.get("json"))
}

fn cmd_start_child(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse(args, &["sheet", "size"]))?;
    usage(args.no_positionals())?;
    let sheet = usage(Sheet::parse(args.get("sheet").unwrap_or("empty")))?;
    harness::start_child(
        sheet,
        usage(parse_size(args.get("size").unwrap_or("960x640")))?,
    )
    .map_err(Failure::Run)
}

fn cmd_idle(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse(
        args,
        &["sheet", "seconds", "settle-ms", "size", "json"],
    ))?;
    usage(args.no_positionals())?;
    let seconds: f64 = usage(args.number("seconds", 10.0))?;
    if !(seconds.is_finite() && seconds > 0.0 && seconds <= 3600.0) {
        return Err(Failure::Usage("--seconds must be in (0, 3600]".into()));
    }
    let settle_ms: u64 = usage(args.number("settle-ms", 1000))?;
    let config = IdleConfig {
        sheet: usage(Sheet::parse(args.get("sheet").unwrap_or("empty")))?,
        size: usage(parse_size(args.get("size").unwrap_or("960x640")))?,
        seconds,
        settle: Duration::from_millis(settle_ms),
    };
    let report = harness::idle(config).map_err(Failure::Run)?;
    finish(&report, args.get("json"))
}

fn cmd_hit(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse_with_flags(
        args,
        &["sheet", "samples", "seed", "json"],
        &["snap"],
    ))?;
    usage(args.no_positionals())?;
    let sheet = args.get("sheet").unwrap_or("10k");
    let samples = positive("samples", usage(args.number("samples", 20_000))?)?;
    let seed = usage(args.number("seed", 42u64))?;
    let report = if args.flag("snap") {
        usage(omnia_bench::hit::run_snap(sheet, samples, seed))?
    } else {
        usage(omnia_bench::hit::run(sheet, samples, seed))?
    };
    finish(&report, args.get("json"))
}

fn cmd_compare(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args = usage(Args::parse(args, &["max-regression", "metrics", "json"]))?;
    let [base_path, new_path] = args.positional.as_slice() else {
        return Err(Failure::Usage(
            "compare needs exactly two report files".into(),
        ));
    };
    let max_regression: f64 = usage(args.number("max-regression", 10.0))?;
    let only: Vec<String> = args
        .get("metrics")
        .map(|list| {
            list.split(',')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let base = usage(read_report(base_path))?;
    let new = usage(read_report(new_path))?;
    let outcome = usage(compare(&base, &new, max_regression, &only))?;

    for warning in &outcome.warnings {
        println!("warning: {warning}");
    }
    for change in &outcome.compared {
        println!(
            "ok          {:<40} {:>12} -> {:>12} ({:+.1}%)",
            change.metric,
            round(change.base),
            round(change.new),
            change.percent()
        );
    }
    for change in &outcome.regressions {
        println!(
            "REGRESSION  {:<40} {:>12} -> {:>12} ({:+.1}% > {max_regression}%)",
            change.metric,
            round(change.base),
            round(change.new),
            change.percent()
        );
    }
    for (metric, _) in &outcome.missing {
        println!("MISSING     {metric}: in the base report, not in the new one");
    }
    if let Some(path) = args.get("json") {
        write_json(path, &outcome.to_json(max_regression, &only))?;
    }
    let checked = outcome.compared.len() + outcome.regressions.len() + outcome.missing.len();
    if outcome.passed() {
        println!("compare: {checked} metrics within {max_regression}%");
        Ok(())
    } else {
        println!(
            "compare: {} regressions, {} missing, of {checked} metrics",
            outcome.regressions.len(),
            outcome.missing.len()
        );
        Err(Failure::Regression)
    }
}

fn read_report(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

fn parse_size(text: &str) -> Result<(f32, f32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or("--size must look like 1280x800")?;
    let parse = |v: &str| {
        v.parse::<u32>()
            .ok()
            .filter(|n| (64..=16_384).contains(n))
            .map(|n| n as f32)
            .ok_or(format!("--size: `{v}` is not a size in 64..=16384"))
    };
    Ok((parse(w)?, parse(h)?))
}

/// Prints the report and writes it to `--json FILE` when given.
fn finish(report: &Value, json_path: Option<&str>) -> Result<(), Failure> {
    let text = serde_json::to_string_pretty(report).expect("reports serialise");
    println!("{text}");
    if let Some(path) = json_path {
        write_json(path, report)?;
    }
    Ok(())
}

fn write_json(path: &str, value: &Value) -> Result<(), Failure> {
    let text = serde_json::to_string_pretty(value).expect("reports serialise");
    std::fs::write(path, format!("{text}\n"))
        .map_err(|e| Failure::Run(format!("write {path}: {e}")))?;
    eprintln!("bench: wrote {path}");
    Ok(())
}

fn cmd_spike(args: impl Iterator<Item = String>) -> Result<(), Failure> {
    let args: Vec<String> = args.collect();
    if args
        .first()
        .is_some_and(|scene| matches!(scene.as_str(), "s7" | "chrome" | "shell-cache"))
    {
        return omnia_bench::spike::run(args).map_err(|e| match e {
            SpikeError::Usage(message) => Failure::Usage(message),
            SpikeError::Run(message) => Failure::Run(message),
        });
    }
    let args = usage(Args::parse(
        args,
        &["count", "mode", "frames", "warmup", "size", "json"],
    ))?;
    let [scene] = args.positional.as_slice() else {
        return Err(Failure::Usage("spike needs s1..s8".into()));
    };
    let config = usage(omnia_bench::spike::Config::parse(
        scene,
        usage(args.number("count", 0))?,
        args.get("mode").unwrap_or("fixed"),
    ))?;
    let report = omnia_bench::spike::run_scene(
        config,
        positive("frames", usage(args.number("frames", 600))?)?,
        positive("warmup", usage(args.number("warmup", 60))?)?,
        usage(parse_size(args.get("size").unwrap_or("3072x1728")))?,
    )
    .map_err(Failure::Run)?;
    finish(&report, args.get("json"))
}
