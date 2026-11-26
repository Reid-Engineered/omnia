//! S0.3 / SS1 benchmark runner: worker-process IPC vs in-process `libngspice`.
//!
//! Usage:
//!   bench_ss1 --lib <path-to-libngspice> --worker <path-to-worker> --out <prefix>
//!             [--reps 10] [--startup-iters 10] [--stream-iters 3] [--cancel-iters 5]
//!             [--batch-size 256]
//!
//! Protocol (fixed before the first timed run, canvas plan §2.1 shape):
//! - `reps` repetitions (default 10), rotating case order each repetition, logged to `*-order.txt`.
//! - Suite 1 (Startup / SB3 < 150 ms):
//!   - `worker-cold-op`: spawn fresh worker + dlopen + OP solve + verified result + shutdown.
//!   - `worker-cold-wien-first`: spawn fresh worker + dlopen + Wien TRAN time-to-first-sample.
//!   - `worker-warm-wien-first`: warm worker reuse + Wien TRAN time-to-first-sample.
//!   - `inproc-wien-first`: in-process `libngspice` + Wien TRAN time-to-first-sample.
//! - Suite 2 (Streaming throughput >= 100 kS/s):
//!   - `worker-stream-tran`: 100k+ points × 2 channels streamed over worker stdout binary IPC.
//!   - `inproc-stream-tran`: exact same 100k+ points × 2 channels via in-process `SendData`.
//! - Suite 3 (Cancel / SB4 < 100 ms, SK11 50 ms grace, 0 orphan processes):
//!   - `worker-cancel-coop-kill`: cooperative `bg_halt` with 50 ms total deadline covering
//!     cancel + clean exit + reap; hard-kills and records hard-kill fallback if exceeded.
//!   - `worker-cancel-hard-kill`: immediate hard kill + reap + PID check.
//!   - `inproc-cancel-bghalt`: in-process `bg_halt` latency baseline.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use omnia_s03_spike::circuits;
use omnia_s03_spike::ngspice::{NgspiceLib, SampleSink};
use omnia_s03_spike::worker_client::{CancelMode, WorkerClient};

struct Args {
    lib: PathBuf,
    worker: PathBuf,
    out: PathBuf,
    reps: usize,
    startup_iters: usize,
    stream_iters: usize,
    cancel_iters: usize,
    batch_size: u32,
}

fn parse_args() -> Args {
    let mut lib = None;
    let mut worker = None;
    let mut out = PathBuf::from("results/ss1");
    let mut reps = 10usize;
    let mut startup_iters = 10usize;
    let mut stream_iters = 3usize;
    let mut cancel_iters = 5usize;
    let mut batch_size = 256u32;

    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let val = v.get(i + 1).cloned().unwrap_or_default();
        match v[i].as_str() {
            "--lib" => lib = Some(PathBuf::from(val)),
            "--worker" => worker = Some(PathBuf::from(val)),
            "--out" => out = PathBuf::from(val),
            "--reps" => reps = val.parse().expect("--reps"),
            "--startup-iters" => startup_iters = val.parse().expect("--startup-iters"),
            "--stream-iters" => stream_iters = val.parse().expect("--stream-iters"),
            "--cancel-iters" => cancel_iters = val.parse().expect("--cancel-iters"),
            "--batch-size" => batch_size = val.parse().expect("--batch-size"),
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    Args {
        lib: lib.expect("--lib is required"),
        worker: worker.expect("--worker is required"),
        out,
        reps,
        startup_iters,
        stream_iters,
        cancel_iters,
        batch_size,
    }
}

struct CountingSink {
    points: Arc<AtomicU64>,
    last_t: f64,
    last_ch0: f64,
    /// Pre-allocated ring buffer simulating `SampleRing` writes (no heap allocs per point).
    ring: Vec<[f64; 3]>,
    write_idx: usize,
}

impl CountingSink {
    fn new(points: Arc<AtomicU64>) -> Self {
        Self {
            points,
            last_t: 0.0,
            last_ch0: 0.0,
            ring: vec![[0.0; 3]; 65_536],
            write_idx: 0,
        }
    }
}

impl SampleSink for CountingSink {
    fn on_point(&mut self, t: f64, values: &[f64]) {
        self.last_t = t;
        let c0 = values.first().copied().unwrap_or(0.0);
        let c1 = values.get(1).copied().unwrap_or(0.0);
        self.last_ch0 = c0;
        let slot = self.write_idx & (self.ring.len() - 1);
        self.ring[slot] = [t, c0, c1];
        self.write_idx = self.write_idx.wrapping_add(1);
        self.points.fetch_add(1, Ordering::Relaxed);
    }
}

fn pct(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

fn median_ns_as_ms(v: &mut [u64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2] as f64 / 1e6
    } else {
        (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0 / 1e6
    }
}

fn median_f64(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|x, y| x.total_cmp(y));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

#[derive(Clone, Copy)]
enum StartupCase {
    WorkerColdOp,
    WorkerColdWienFirst,
    WorkerWarmWienFirst,
    InprocWienFirst,
}

impl StartupCase {
    const ALL: [StartupCase; 4] = [
        StartupCase::WorkerColdOp,
        StartupCase::WorkerColdWienFirst,
        StartupCase::WorkerWarmWienFirst,
        StartupCase::InprocWienFirst,
    ];

    fn name(self) -> &'static str {
        match self {
            StartupCase::WorkerColdOp => "worker-cold-op",
            StartupCase::WorkerColdWienFirst => "worker-cold-wien-first",
            StartupCase::WorkerWarmWienFirst => "worker-warm-wien-first",
            StartupCase::InprocWienFirst => "inproc-wien-first",
        }
    }
}

#[derive(Clone, Copy)]
enum StreamCase {
    WorkerStream,
    InprocStream,
}

impl StreamCase {
    const ALL: [StreamCase; 2] = [StreamCase::WorkerStream, StreamCase::InprocStream];

    fn name(self) -> &'static str {
        match self {
            StreamCase::WorkerStream => "worker-stream-tran",
            StreamCase::InprocStream => "inproc-stream-tran",
        }
    }
}

#[derive(Clone, Copy)]
enum CancelCase {
    WorkerCoopThenKill,
    WorkerHardKill,
    InprocBgHalt,
}

impl CancelCase {
    const ALL: [CancelCase; 3] = [
        CancelCase::WorkerCoopThenKill,
        CancelCase::WorkerHardKill,
        CancelCase::InprocBgHalt,
    ];

    fn name(self) -> &'static str {
        match self {
            CancelCase::WorkerCoopThenKill => "worker-cancel-coop-kill",
            CancelCase::WorkerHardKill => "worker-cancel-hard-kill",
            CancelCase::InprocBgHalt => "inproc-cancel-bghalt",
        }
    }
}

fn verify_op_values(vals: &[f64]) {
    assert_eq!(vals.len(), 2, "expected 2 OP probes");
    let exp0 = 10.0 * 2000.0 / 3000.0;
    let exp1 = 10.0 * 4000.0 / 7000.0;
    assert!(
        (vals[0] - exp0).abs() < 1e-6,
        "v(n1) mismatch: got {}, expected {exp0}",
        vals[0]
    );
    assert!(
        (vals[1] - exp1).abs() < 1e-6,
        "v(n2) mismatch: got {}, expected {exp1}",
        vals[1]
    );
}

fn run_startup_iter(
    case: StartupCase,
    worker_bin: &Path,
    lib_path: &Path,
    warm_worker: &mut WorkerClient,
    inproc: &mut NgspiceLib,
    batch_size: u32,
) -> (u64, u64) {
    let op_net = circuits::simple_op_netlist();
    let op_probes = vec!["n1".into(), "n2".into()];
    let wien_net = circuits::wien_tran_netlist();
    let wien_probes = vec!["vout".into(), "nplus".into()];

    match case {
        StartupCase::WorkerColdOp => {
            let t0 = Instant::now();
            let (mut w, ready_dur) =
                WorkerClient::spawn(worker_bin, lib_path).expect("spawn worker");
            let vals = w.run_op(op_net, op_probes).expect("worker op");
            let elapsed = t0.elapsed().as_nanos() as u64;
            verify_op_values(&vals);
            w.shutdown().expect("shutdown");
            (elapsed, ready_dur.as_nanos() as u64)
        }
        StartupCase::WorkerColdWienFirst => {
            let t0 = Instant::now();
            let (mut w, ready_dur) =
                WorkerClient::spawn(worker_bin, lib_path).expect("spawn worker");
            let res = w
                .run_tran(wien_net, wien_probes, batch_size)
                .expect("worker wien");
            let first = res
                .first_sample_at
                .expect("wien first sample")
                .duration_since(t0)
                .as_nanos() as u64;
            assert!(res.total_points > 100, "wien points={}", res.total_points);
            w.shutdown().expect("shutdown");
            (first, ready_dur.as_nanos() as u64)
        }
        StartupCase::WorkerWarmWienFirst => {
            let t0 = Instant::now();
            let res = warm_worker
                .run_tran(wien_net, wien_probes, batch_size)
                .expect("warm worker wien");
            let first = res
                .first_sample_at
                .expect("warm wien first sample")
                .duration_since(t0)
                .as_nanos() as u64;
            assert!(res.total_points > 100);
            (first, 0)
        }
        StartupCase::InprocWienFirst => {
            let counter = Arc::new(AtomicU64::new(0));
            let t0 = Instant::now();
            let stats = inproc
                .run_tran_sync(
                    &wien_net,
                    &wien_probes,
                    Box::new(CountingSink::new(counter)),
                )
                .expect("inproc wien");
            let first = stats
                .first_sample_at
                .expect("inproc wien first sample")
                .duration_since(t0)
                .as_nanos() as u64;
            assert!(stats.total_points > 100);
            (first, 0)
        }
    }
}

fn main() {
    let a = parse_args();

    // Measure one-time in-process dlopen + ngSpice_Init cost.
    let t_dlopen = Instant::now();
    let mut inproc = NgspiceLib::load(&a.lib).expect("load inproc libngspice");
    let inproc_dlopen_ns = t_dlopen.elapsed().as_nanos() as u64;

    // Sanity check OP accuracy on both modes before timing.
    let op_vals = inproc
        .run_op(&circuits::simple_op_netlist(), &["n1".into(), "n2".into()])
        .expect("inproc op sanity");
    verify_op_values(&op_vals);

    let (mut warm_worker, first_spawn_ready) =
        WorkerClient::spawn(&a.worker, &a.lib).expect("spawn warm worker");
    let worker_op_vals = warm_worker
        .run_op(
            circuits::simple_op_netlist(),
            vec!["n1".into(), "n2".into()],
        )
        .expect("worker op sanity");
    verify_op_values(&worker_op_vals);

    let mut csv = String::from(
        "suite,rep,case,iter,ns,aux_ns,points,channel_samples,throughput_pts_s,throughput_ch_s,coop_stop,hard_kill,orphan\n",
    );
    let mut order_log = String::new();

    // --- Suite 1: Startup & time-to-first-sample (SB3) ---
    let mut s1_p99s: Vec<Vec<u64>> = vec![Vec::new(); StartupCase::ALL.len()];
    let mut s1_all: Vec<Vec<u64>> = vec![Vec::new(); StartupCase::ALL.len()];
    let mut s1_spawn_ready_all: Vec<u64> = Vec::new();
    let mut per_run_json = String::new();

    for rep in 0..a.reps {
        let order: Vec<usize> = (0..StartupCase::ALL.len())
            .map(|k| (k + rep) % StartupCase::ALL.len())
            .collect();
        let _ = writeln!(
            order_log,
            "startup rep {rep}: {}",
            order
                .iter()
                .map(|&k| StartupCase::ALL[k].name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        for &k in &order {
            let c = StartupCase::ALL[k];
            if matches!(
                c,
                StartupCase::WorkerWarmWienFirst | StartupCase::InprocWienFirst
            ) {
                let _ = run_startup_iter(
                    c,
                    &a.worker,
                    &a.lib,
                    &mut warm_worker,
                    &mut inproc,
                    a.batch_size,
                );
            }
            let mut s = Vec::with_capacity(a.startup_iters);
            for it in 0..a.startup_iters {
                let (ns, aux_ns) = run_startup_iter(
                    c,
                    &a.worker,
                    &a.lib,
                    &mut warm_worker,
                    &mut inproc,
                    a.batch_size,
                );
                s.push(ns);
                if aux_ns > 0 {
                    s1_spawn_ready_all.push(aux_ns);
                }
                let _ = writeln!(
                    csv,
                    "startup,{rep},{},{it},{ns},{aux_ns},0,0,0,0,0,0,0",
                    c.name()
                );
            }
            s1_all[k].extend_from_slice(&s);
            let p50_ms = median_ns_as_ms(&mut s);
            let p99 = pct(&s, 99.0);
            let max = *s.last().unwrap_or(&0);
            s1_p99s[k].push(p99);
            let _ = writeln!(
                per_run_json,
                "      {{\"suite\": \"startup\", \"rep\": {rep}, \"case\": \"{}\", \"p50_ms\": {:.4}, \"p99_ms\": {:.4}, \"max_ms\": {:.4}}},",
                c.name(),
                p50_ms,
                ms(p99),
                ms(max)
            );
        }
    }

    // --- Suite 2: Binary double streaming throughput (>= 100 kS/s) ---
    let stream_net = circuits::stream_tran_netlist();
    let stream_probes = vec!["vin".into(), "vout".into()];
    let mut s2_p99s: Vec<Vec<u64>> = vec![Vec::new(); StreamCase::ALL.len()];
    let mut s2_all_ns: Vec<Vec<u64>> = vec![Vec::new(); StreamCase::ALL.len()];
    let mut s2_pts_s: Vec<Vec<f64>> = vec![Vec::new(); StreamCase::ALL.len()];
    let mut s2_ch_s: Vec<Vec<f64>> = vec![Vec::new(); StreamCase::ALL.len()];
    let mut stream_points_seen = 0u64;

    for rep in 0..a.reps {
        let order: Vec<usize> = (0..StreamCase::ALL.len())
            .map(|k| (k + rep) % StreamCase::ALL.len())
            .collect();
        let _ = writeln!(
            order_log,
            "stream rep {rep}: {}",
            order
                .iter()
                .map(|&k| StreamCase::ALL[k].name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        for &k in &order {
            let c = StreamCase::ALL[k];
            let mut s = Vec::with_capacity(a.stream_iters);
            for it in 0..a.stream_iters {
                let t0 = Instant::now();
                let (pts, ch_samples) = match c {
                    StreamCase::WorkerStream => {
                        let res = warm_worker
                            .run_tran(stream_net.clone(), stream_probes.clone(), a.batch_size)
                            .expect("worker stream");
                        (res.total_points, res.total_channel_samples)
                    }
                    StreamCase::InprocStream => {
                        let counter = Arc::new(AtomicU64::new(0));
                        let stats = inproc
                            .run_tran_sync(
                                &stream_net,
                                &stream_probes,
                                Box::new(CountingSink::new(counter)),
                            )
                            .expect("inproc stream");
                        (stats.total_points, stats.total_points * 2)
                    }
                };
                let ns = t0.elapsed().as_nanos() as u64;
                let secs = ns as f64 / 1e9;
                let pts_s = pts as f64 / secs;
                let ch_s = ch_samples as f64 / secs;
                stream_points_seen = pts;
                s.push(ns);
                s2_pts_s[k].push(pts_s);
                s2_ch_s[k].push(ch_s);
                let _ = writeln!(
                    csv,
                    "stream,{rep},{},{it},{ns},0,{pts},{ch_samples},{pts_s:.1},{ch_s:.1},0,0,0",
                    c.name()
                );
            }
            s2_all_ns[k].extend_from_slice(&s);
            let p50_ms = median_ns_as_ms(&mut s);
            let p99 = pct(&s, 99.0);
            let max = *s.last().unwrap_or(&0);
            s2_p99s[k].push(p99);
            let _ = writeln!(
                per_run_json,
                "      {{\"suite\": \"stream\", \"rep\": {rep}, \"case\": \"{}\", \"p50_ms\": {:.4}, \"p99_ms\": {:.4}, \"max_ms\": {:.4}}},",
                c.name(),
                p50_ms,
                ms(p99),
                ms(max)
            );
        }
    }

    warm_worker.shutdown().expect("shutdown warm worker");

    // --- Suite 3: Cancel / Kill latency & orphan process audit (SB4, SK11) ---
    let heavy_net = circuits::heavy_cancel_tran_netlist();
    let heavy_probes = vec!["n0".into(), "n24".into()];
    let grace = Duration::from_millis(50);
    let mut s3_p99s: Vec<Vec<u64>> = vec![Vec::new(); CancelCase::ALL.len()];
    let mut s3_all_ns: Vec<Vec<u64>> = vec![Vec::new(); CancelCase::ALL.len()];
    let mut total_orphans = 0u64;
    let mut coop_stops = 0u64;
    let mut late_coop_stops = 0u64;
    let mut hard_kills = 0u64;

    for rep in 0..a.reps {
        let order: Vec<usize> = (0..CancelCase::ALL.len())
            .map(|k| (k + rep) % CancelCase::ALL.len())
            .collect();
        let _ = writeln!(
            order_log,
            "cancel rep {rep}: {}",
            order
                .iter()
                .map(|&k| CancelCase::ALL[k].name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        for &k in &order {
            let c = CancelCase::ALL[k];
            let mut s = Vec::with_capacity(a.cancel_iters);
            for it in 0..a.cancel_iters {
                match c {
                    CancelCase::WorkerCoopThenKill | CancelCase::WorkerHardKill => {
                        let hard_only = matches!(c, CancelCase::WorkerHardKill);
                        let (w, _) =
                            WorkerClient::spawn(&a.worker, &a.lib).expect("spawn cancel worker");
                        let outcome = w
                            .run_tran_then_cancel(
                                heavy_net.clone(),
                                heavy_probes.clone(),
                                64,
                                50,
                                grace,
                                hard_only,
                            )
                            .expect("worker cancel");
                        let ns = outcome.cancel_elapsed.as_nanos() as u64;
                        s.push(ns);
                        let is_coop = outcome.mode == CancelMode::CooperativeReaped;
                        let is_hard = outcome.mode == CancelMode::HardKilled;
                        if !hard_only {
                            match outcome.mode {
                                CancelMode::CooperativeReaped => coop_stops += 1,
                                CancelMode::CooperativeLateReaped => late_coop_stops += 1,
                                CancelMode::HardKilled => hard_kills += 1,
                            }
                        }
                        if outcome.pid_alive_after {
                            total_orphans += 1;
                        }
                        let _ = writeln!(
                            csv,
                            "cancel,{rep},{},{it},{ns},0,0,0,0,0,{},{},{}",
                            c.name(),
                            u8::from(is_coop),
                            u8::from(is_hard),
                            u8::from(outcome.pid_alive_after)
                        );
                    }
                    CancelCase::InprocBgHalt => {
                        let counter = Arc::new(AtomicU64::new(0));
                        inproc
                            .start_tran_bg(
                                &heavy_net,
                                &heavy_probes,
                                Box::new(CountingSink::new(Arc::clone(&counter))),
                            )
                            .expect("inproc bg start");
                        let wait_start = Instant::now();
                        while counter.load(Ordering::Relaxed) < 50 {
                            assert!(
                                wait_start.elapsed() < Duration::from_secs(10),
                                "inproc heavy tran failed to emit 50 points"
                            );
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        let t0 = Instant::now();
                        inproc.bg_halt().expect("inproc bg_halt");
                        let _ = inproc
                            .wait_tran_bg(Duration::from_secs(5))
                            .expect("wait bg_halt");
                        let ns = t0.elapsed().as_nanos() as u64;
                        s.push(ns);
                        let _ =
                            writeln!(csv, "cancel,{rep},{},{it},{ns},0,0,0,0,0,1,0,0", c.name());
                    }
                }
            }
            s3_all_ns[k].extend_from_slice(&s);
            let p50_ms = median_ns_as_ms(&mut s);
            let p99 = pct(&s, 99.0);
            let max = *s.last().unwrap_or(&0);
            s3_p99s[k].push(p99);
            let _ = writeln!(
                per_run_json,
                "      {{\"suite\": \"cancel\", \"rep\": {rep}, \"case\": \"{}\", \"p50_ms\": {:.4}, \"p99_ms\": {:.4}, \"max_ms\": {:.4}}},",
                c.name(),
                p50_ms,
                ms(p99),
                ms(max)
            );
        }
    }

    assert_eq!(
        total_orphans, 0,
        "SINV5 violated: {total_orphans} orphan worker processes remained alive"
    );

    // --- Summaries ---
    let mut summary_entries = Vec::new();
    let mut table = String::new();

    for (k, c) in StartupCase::ALL.iter().enumerate() {
        let mut runs = s1_p99s[k].clone();
        let med_ms = median_ns_as_ms(&mut runs);
        let (lo, hi) = (runs[0], *runs.last().unwrap_or(&0));
        let mut all = s1_all[k].clone();
        let all_p50_ms = median_ns_as_ms(&mut all);
        summary_entries.push(format!(
            "    {{\"suite\": \"startup\", \"case\": \"{}\", \"median_run_p99_ms\": {:.4}, \"run_p99_min_ms\": {:.4}, \"run_p99_max_ms\": {:.4}, \"all_p50_ms\": {:.4}, \"all_p99_ms\": {:.4}, \"all_max_ms\": {:.4}, \"samples\": {}}}",
            c.name(),
            med_ms,
            ms(lo),
            ms(hi),
            all_p50_ms,
            ms(pct(&all, 99.0)),
            ms(*all.last().unwrap_or(&0)),
            all.len(),
        ));
        let _ = writeln!(
            table,
            "{:<26} median run p99 {:>8.3} ms  range {:>7.3}-{:<7.3} ms  p50 {:>7.3} ms",
            c.name(),
            med_ms,
            ms(lo),
            ms(hi),
            all_p50_ms,
        );
    }

    for (k, c) in StreamCase::ALL.iter().enumerate() {
        let mut runs = s2_p99s[k].clone();
        let med_ms = median_ns_as_ms(&mut runs);
        let (lo, hi) = (runs[0], *runs.last().unwrap_or(&0));
        let mut all = s2_all_ns[k].clone();
        let all_p50_ms = median_ns_as_ms(&mut all);
        let mut pts_s = s2_pts_s[k].clone();
        let med_pts_s = median_f64(&mut pts_s);
        let mut ch_s = s2_ch_s[k].clone();
        let med_ch_s = median_f64(&mut ch_s);
        summary_entries.push(format!(
            "    {{\"suite\": \"stream\", \"case\": \"{}\", \"points\": {}, \"median_run_p99_ms\": {:.4}, \"run_p99_min_ms\": {:.4}, \"run_p99_max_ms\": {:.4}, \"all_p50_ms\": {:.4}, \"median_points_per_sec\": {:.1}, \"median_channel_samples_per_sec\": {:.1}, \"samples\": {}}}",
            c.name(),
            stream_points_seen,
            med_ms,
            ms(lo),
            ms(hi),
            all_p50_ms,
            med_pts_s,
            med_ch_s,
            all.len(),
        ));
        let _ = writeln!(
            table,
            "{:<26} median run p99 {:>8.3} ms  p50 {:>7.3} ms  rate {:>9.0} pts/s ({:.0} ch-S/s)",
            c.name(),
            med_ms,
            all_p50_ms,
            med_pts_s,
            med_ch_s,
        );
    }

    for (k, c) in CancelCase::ALL.iter().enumerate() {
        let mut runs = s3_p99s[k].clone();
        let med_ms = median_ns_as_ms(&mut runs);
        let (lo, hi) = (runs[0], *runs.last().unwrap_or(&0));
        let mut all = s3_all_ns[k].clone();
        let all_p50_ms = median_ns_as_ms(&mut all);
        summary_entries.push(format!(
            "    {{\"suite\": \"cancel\", \"case\": \"{}\", \"median_run_p99_ms\": {:.4}, \"run_p99_min_ms\": {:.4}, \"run_p99_max_ms\": {:.4}, \"all_p50_ms\": {:.4}, \"all_p99_ms\": {:.4}, \"all_max_ms\": {:.4}, \"samples\": {}}}",
            c.name(),
            med_ms,
            ms(lo),
            ms(hi),
            all_p50_ms,
            ms(pct(&all, 99.0)),
            ms(*all.last().unwrap_or(&0)),
            all.len(),
        ));
        let _ = writeln!(
            table,
            "{:<26} median run p99 {:>8.3} ms  range {:>7.3}-{:<7.3} ms  p50 {:>7.3} ms",
            c.name(),
            med_ms,
            ms(lo),
            ms(hi),
            all_p50_ms,
        );
    }

    let spawn_ready_p50_ms = median_ns_as_ms(&mut s1_spawn_ready_all);
    let spawn_ready_p99_ms = ms(pct(&s1_spawn_ready_all, 99.0));

    let per_run_trimmed = per_run_json.trim_end().trim_end_matches(',');
    let json = format!(
        "{{\n  \"spike\": \"SS1\",\n  \"budgets\": [\"SB3\", \"SB4\"],\n  \"reps\": {},\n  \"startup_iters\": {},\n  \"stream_iters\": {},\n  \"cancel_iters\": {},\n  \"batch_size\": {},\n  \"inproc_dlopen_init_ms\": {:.4},\n  \"first_worker_spawn_ready_ms\": {:.4},\n  \"worker_spawn_ready_p50_ms\": {:.4},\n  \"worker_spawn_ready_p99_ms\": {:.4},\n  \"stream_points_per_run\": {},\n  \"cancel_coop_stops\": {},\n  \"cancel_late_coop_stops\": {},\n  \"cancel_hard_kill_fallbacks\": {},\n  \"orphan_processes\": {},\n  \"cases\": [\n{}\n  ],\n  \"runs\": [\n{}\n  ]\n}}\n",
        a.reps,
        a.startup_iters,
        a.stream_iters,
        a.cancel_iters,
        a.batch_size,
        ms(inproc_dlopen_ns),
        ms(first_spawn_ready.as_nanos() as u64),
        spawn_ready_p50_ms,
        spawn_ready_p99_ms,
        stream_points_seen,
        coop_stops,
        late_coop_stops,
        hard_kills,
        total_orphans,
        summary_entries.join(",\n"),
        per_run_trimmed,
    );

    if let Some(p) = a.out.parent() {
        std::fs::create_dir_all(p).expect("out dir");
    }
    let stem = a.out.to_string_lossy().into_owned();
    std::fs::write(format!("{stem}.json"), json).expect("write json");
    std::fs::write(format!("{stem}.csv"), csv).expect("write csv");
    std::fs::write(format!("{stem}-order.txt"), order_log).expect("write order");

    print!(
        "SS1 complete (reps={}, batch={}, inproc_dlopen={:.2} ms, spawn_ready p50={:.2} ms p99={:.2} ms, coop_stops={}, late_coop_stops={}, hard_fallbacks={}, orphans={})\n{table}",
        a.reps,
        a.batch_size,
        ms(inproc_dlopen_ns),
        spawn_ready_p50_ms,
        spawn_ready_p99_ms,
        coop_stops,
        late_coop_stops,
        hard_kills,
        total_orphans
    );
}
