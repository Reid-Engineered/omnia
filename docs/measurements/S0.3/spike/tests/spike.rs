//! Unit and integration tests for the S0.3 / SS1 spike crate.
//!
//! Protocol, bounds, and singleton-guard unit tests always run. Live `libngspice` tests run
//! whenever `NGSPICE_LIB` points to a built `ngspice` shared library.

use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use omnia_s03_spike::circuits;
use omnia_s03_spike::ngspice::{InstanceGuard, NgspiceLib, SampleSink};
use omnia_s03_spike::protocol::{
    MAX_BATCH_POINTS, MAX_CHANNELS, Request, Response, TAG_OP_RESULT, TAG_RUN_OP, TAG_RUN_TRAN,
    TAG_SAMPLE_BATCH, read_request, read_response, write_request, write_response,
    write_sample_batch_raw,
};
use omnia_s03_spike::worker_client::{CancelMode, WorkerClient, is_pid_alive};

#[test]
fn protocol_request_roundtrip() {
    let cases = vec![
        Request::Ping,
        Request::RunOp {
            probes: vec!["n1".into(), "n2".into()],
            netlist: circuits::simple_op_netlist(),
        },
        Request::RunTran {
            probes: vec!["vout".into(), "nplus".into()],
            netlist: circuits::wien_tran_netlist(),
            batch_size: 256,
        },
        Request::Cancel,
        Request::Shutdown,
    ];
    let mut wire = Vec::new();
    for req in &cases {
        write_request(&mut wire, req).expect("write request");
    }
    let mut r = Cursor::new(wire);
    let mut buf = Vec::new();
    for expected in &cases {
        let got = read_request(&mut r, &mut buf)
            .expect("read request")
            .expect("frame present");
        assert_eq!(&got, expected);
    }
    assert!(read_request(&mut r, &mut buf).expect("eof").is_none());
}

#[test]
fn protocol_response_and_raw_batch_roundtrip() {
    let mut wire = Vec::new();
    write_response(&mut wire, &Response::Ready).unwrap();
    write_response(
        &mut wire,
        &Response::OpResult {
            values: vec![6.666_666_7, 5.714_285_7],
        },
    )
    .unwrap();
    let mut scratch = Vec::new();
    write_sample_batch_raw(
        &mut wire,
        2,
        2,
        &[0.0, 1.0, 2.0, 1e-6, 1.5, 2.5],
        &mut scratch,
    )
    .unwrap();
    write_response(&mut wire, &Response::RunDone { total_points: 2 }).unwrap();
    write_response(&mut wire, &Response::Cancelled { total_points: 1 }).unwrap();
    write_response(
        &mut wire,
        &Response::Error {
            message: "missing probe".into(),
        },
    )
    .unwrap();

    let mut r = Cursor::new(wire);
    let mut buf = Vec::new();
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::Ready)
    );
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::OpResult {
            values: vec![6.666_666_7, 5.714_285_7]
        })
    );
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::SampleBatch {
            channels: 2,
            points: 2,
            data: vec![0.0, 1.0, 2.0, 1e-6, 1.5, 2.5]
        })
    );
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::RunDone { total_points: 2 })
    );
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::Cancelled { total_points: 1 })
    );
    assert_eq!(
        read_response(&mut r, &mut buf).unwrap(),
        Some(Response::Error {
            message: "missing probe".into()
        })
    );
}

fn raw_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[test]
fn protocol_rejects_oversized_counts_and_batch_sizes() {
    let mut buf = Vec::new();

    // 1. Crafted OpResult declaring u32::MAX values in a 4-byte payload.
    let bad_op = raw_frame(TAG_OP_RESULT, &u32::MAX.to_le_bytes());
    let err = read_response(&mut Cursor::new(bad_op), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // 2. Crafted SampleBatch declaring huge points with 6-byte payload.
    let mut bad_batch_payload = Vec::new();
    bad_batch_payload.extend_from_slice(&2u16.to_le_bytes());
    bad_batch_payload.extend_from_slice(&u32::MAX.to_le_bytes());
    let bad_batch = raw_frame(TAG_SAMPLE_BATCH, &bad_batch_payload);
    let err = read_response(&mut Cursor::new(bad_batch), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // 3. Crafted SampleBatch with valid points (100) but mismatched/truncated payload (only 16 bytes).
    let mut trunc_batch_payload = Vec::new();
    trunc_batch_payload.extend_from_slice(&2u16.to_le_bytes());
    trunc_batch_payload.extend_from_slice(&100u32.to_le_bytes());
    trunc_batch_payload.extend_from_slice(&[0u8; 16]);
    let trunc_batch = raw_frame(TAG_SAMPLE_BATCH, &trunc_batch_payload);
    let err = read_response(&mut Cursor::new(trunc_batch), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // 4. Crafted RunOp declaring u32::MAX strings in a 4-byte payload.
    let bad_str_list = raw_frame(TAG_RUN_OP, &u32::MAX.to_le_bytes());
    let err = read_request(&mut Cursor::new(bad_str_list), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // 5. Crafted RunTran with batch_size = 0 and batch_size > MAX_BATCH_POINTS.
    for bad_bs in [0u32, MAX_BATCH_POINTS + 1, u32::MAX] {
        let mut p = Vec::new();
        p.extend_from_slice(&bad_bs.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        let frame = raw_frame(TAG_RUN_TRAN, &p);
        let err = read_request(&mut Cursor::new(frame), &mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    fn push_str_list(out: &mut Vec<u8>, items: &[String]) {
        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
        for s in items {
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
    }

    let too_many_probes: Vec<String> = (0..=(MAX_CHANNELS as usize))
        .map(|i| format!("v{i}"))
        .collect();
    let dummy_net = vec!["* net".to_string(), ".end".to_string()];

    // 6. Complete RunOp payload declaring 65 (> MAX_CHANNELS) valid probes.
    let mut op_65 = Vec::new();
    push_str_list(&mut op_65, &too_many_probes);
    push_str_list(&mut op_65, &dummy_net);
    let err = read_request(&mut Cursor::new(raw_frame(TAG_RUN_OP, &op_65)), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // 7. Complete RunTran payload declaring 65 (> MAX_CHANNELS) valid probes.
    let mut tran_65 = Vec::new();
    tran_65.extend_from_slice(&64u32.to_le_bytes());
    push_str_list(&mut tran_65, &too_many_probes);
    push_str_list(&mut tran_65, &dummy_net);
    let err = read_request(
        &mut Cursor::new(raw_frame(TAG_RUN_TRAN, &tran_65)),
        &mut buf,
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn instance_guard_enforces_single_active_owner() {
    let g1 = InstanceGuard::acquire().expect("first acquire succeeds");
    let err = InstanceGuard::acquire().expect_err("second concurrent acquire must fail");
    assert!(err.contains("already loaded"), "unexpected err: {err}");
    drop(g1);
    let g2 = InstanceGuard::acquire().expect("acquire after drop succeeds");
    drop(g2);
}

#[test]
fn pid_liveness_reports_current_process_alive() {
    let alive = is_pid_alive(std::process::id()).expect("query own pid");
    assert!(alive);
}

struct NopSink(Arc<AtomicU64>);
impl SampleSink for NopSink {
    fn on_point(&mut self, _t: f64, _values: &[f64]) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn live_worker_op_tran_missing_probe_cancel_and_lifecycle() {
    let Ok(lib) = std::env::var("NGSPICE_LIB") else {
        return;
    };
    let lib_path = PathBuf::from(lib);
    let worker_bin = PathBuf::from(env!("CARGO_BIN_EXE_omnia-ngspice-worker"));

    // 1. Worker OP accuracy + SINV7 missing probe refusal.
    let (mut w, _) = WorkerClient::spawn(&worker_bin, &lib_path).expect("spawn");
    let vals = w
        .run_op(
            circuits::simple_op_netlist(),
            vec!["n1".into(), "n2".into()],
        )
        .expect("op");
    assert!((vals[0] - 20.0 / 3.0).abs() < 1e-6);
    assert!((vals[1] - 40.0 / 7.0).abs() < 1e-6);

    let err = w
        .run_op(circuits::simple_op_netlist(), vec!["does_not_exist".into()])
        .expect_err("SINV7: missing probe must error");
    assert!(err.contains("SINV7"), "unexpected err: {err}");

    // 2. Wien TRAN streaming.
    let tran = w
        .run_tran(
            circuits::wien_tran_netlist(),
            vec!["vout".into(), "nplus".into()],
            128,
        )
        .expect("wien tran");
    assert!(tran.total_points > 500, "points={}", tran.total_points);
    w.shutdown().expect("shutdown");

    // 3. Cancel + zero orphan check (SB4 / SINV5), enforcing 50 ms total deadline.
    let (w2, _) = WorkerClient::spawn(&worker_bin, &lib_path).expect("spawn 2");
    let outcome = w2
        .run_tran_then_cancel(
            circuits::heavy_cancel_tran_netlist(),
            vec!["n0".into(), "n24".into()],
            64,
            20,
            Duration::from_millis(50),
            false,
        )
        .expect("cancel");
    assert!(!outcome.pid_alive_after, "SINV5: orphan worker survived");
    assert!(
        outcome.mode == CancelMode::CooperativeReaped || outcome.mode == CancelMode::HardKilled
    );
    assert!(
        outcome.cancel_elapsed < Duration::from_millis(100),
        "SB4 exceeded: {:?}",
        outcome.cancel_elapsed
    );

    // 4. Sync-only instance drops cleanly and can reload; after `bg_run` starts, `Drop`
    //    halts & detaches and pins the library + singleton guard for the process lifetime.
    {
        let mut ng_sync = NgspiceLib::load(&lib_path).expect("load sync-only instance");
        let second = NgspiceLib::load(&lib_path);
        assert!(
            second.is_err(),
            "second concurrent NgspiceLib load must fail"
        );
        let op = ng_sync
            .run_op(&circuits::simple_op_netlist(), &["n1".into(), "n2".into()])
            .expect("sync op");
        assert!((op[0] - 20.0 / 3.0).abs() < 1e-6);
    }
    {
        let mut ng = NgspiceLib::load(&lib_path).expect("reload after sync-only Drop must succeed");
        let count = Arc::new(AtomicU64::new(0));
        ng.start_tran_bg(
            &circuits::heavy_cancel_tran_netlist(),
            &["n0".into(), "n24".into()],
            Box::new(NopSink(Arc::clone(&count))),
        )
        .expect("start bg tran");
        while count.load(Ordering::Relaxed) < 10 {
            std::thread::sleep(Duration::from_millis(1));
        }
        // Drop `ng` while bg_run is actively streaming — Drop halts & pins library + guard.
    }
    assert!(
        NgspiceLib::load(&lib_path).is_err(),
        "background thread usage pins library and singleton guard safely"
    );
}
