//! Unit and integration tests for the S0.3 / SS1 spike crate.
//!
//! Protocol and circuit tests always run. Live `libngspice` tests run whenever `NGSPICE_LIB`
//! points to a built `ngspice` shared library.

use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use omnia_s03_spike::circuits;
use omnia_s03_spike::protocol::{
    Request, Response, read_request, read_response, write_request, write_response,
    write_sample_batch_raw,
};
use omnia_s03_spike::worker_client::WorkerClient;

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

#[test]
fn live_worker_op_tran_missing_probe_and_cancel() {
    let Ok(lib) = std::env::var("NGSPICE_LIB") else {
        return;
    };
    let lib_path = PathBuf::from(lib);
    let worker_bin = PathBuf::from(env!("CARGO_BIN_EXE_omnia-ngspice-worker"));

    // 1. OP accuracy + SINV7 missing probe refusal.
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

    // 3. Cancel + zero orphan check (SB4 / SINV5).
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
}
