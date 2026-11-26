//! Prototype isolated `omnia-ngspice-worker` executable (`SS1` / `SD5`).
//!
//! Dynamically loads `libngspice` exclusively on the main thread (`!Sync`), speaks binary
//! framed IPC on stdin/stdout, streams transient `(t, ch0, ...)` double frames directly from
//! `SendData`, and honours cooperative `Cancel` via a dedicated stdin reader thread that
//! wakes the single `NgspiceLib` owner thread through `BgWakeup`.

use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omnia_s03_spike::ngspice::{NgspiceLib, SampleSink};
use omnia_s03_spike::protocol::{
    MAX_BATCH_POINTS, MAX_CHANNELS, Request, Response, read_request, validate_batch_size,
    write_response, write_sample_batch_raw,
};

struct StreamingSink {
    stdout: Arc<Mutex<BufWriter<io::Stdout>>>,
    channels: u16,
    batch_size: usize,
    first_flushed: bool,
    f64_buf: Vec<f64>,
    wire_scratch: Vec<u8>,
    cancelled: Arc<AtomicBool>,
}

impl StreamingSink {
    fn new(
        stdout: Arc<Mutex<BufWriter<io::Stdout>>>,
        channels: u16,
        batch_size: u32,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        if channels > MAX_CHANNELS {
            return Err(format!(
                "channel count {channels} exceeds MAX_CHANNELS ({MAX_CHANNELS})"
            ));
        }
        let bs = validate_batch_size(batch_size).map_err(|e| {
            format!("invalid batch_size ({batch_size}, max {MAX_BATCH_POINTS}): {e}")
        })? as usize;
        let stride = 1 + channels as usize;
        let f64_cap = bs
            .checked_mul(stride)
            .ok_or("sample buffer capacity overflow")?;
        let wire_cap = f64_cap
            .checked_mul(8)
            .and_then(|b| b.checked_add(6))
            .ok_or("wire scratch capacity overflow")?;
        Ok(Self {
            stdout,
            channels,
            batch_size: bs,
            first_flushed: false,
            f64_buf: Vec::with_capacity(f64_cap),
            wire_scratch: Vec::with_capacity(wire_cap),
            cancelled,
        })
    }

    fn flush_batch(&mut self, flush_writer: bool) {
        if self.f64_buf.is_empty() {
            return;
        }
        let stride = 1 + self.channels as usize;
        let points = (self.f64_buf.len() / stride) as u32;
        if let Ok(mut out) = self.stdout.lock() {
            let _ = write_sample_batch_raw(
                &mut *out,
                self.channels,
                points,
                &self.f64_buf,
                &mut self.wire_scratch,
            );
            if flush_writer {
                let _ = out.flush();
            }
        }
        self.f64_buf.clear();
    }
}

impl SampleSink for StreamingSink {
    fn on_point(&mut self, t: f64, values: &[f64]) {
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        self.f64_buf.push(t);
        self.f64_buf.extend_from_slice(values);
        let stride = 1 + self.channels as usize;
        let buffered_points = self.f64_buf.len() / stride;
        // Flush the very first point immediately so host observes honest SB3 time-to-first-sample.
        if !self.first_flushed {
            self.first_flushed = true;
            self.flush_batch(true);
        } else if buffered_points >= self.batch_size {
            self.flush_batch(true);
        }
    }
}

impl Drop for StreamingSink {
    fn drop(&mut self) {
        if !self.cancelled.load(Ordering::Relaxed) {
            self.flush_batch(true);
        }
    }
}

fn parse_lib_arg() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--lib" {
            let p = args.next().ok_or("--lib requires a path")?;
            return Ok(PathBuf::from(p));
        }
    }
    if let Ok(p) = std::env::var("NGSPICE_LIB") {
        return Ok(PathBuf::from(p));
    }
    Err("usage: omnia-ngspice-worker --lib <path-to-libngspice>".into())
}

fn main() {
    let stdout = Arc::new(Mutex::new(BufWriter::with_capacity(
        64 * 1024,
        io::stdout(),
    )));
    let lib_path = match parse_lib_arg() {
        Ok(p) => p,
        Err(msg) => {
            if let Ok(mut out) = stdout.lock() {
                let _ = write_response(&mut *out, &Response::Error { message: msg });
            }
            std::process::exit(2);
        }
    };

    let mut ng = match NgspiceLib::load(&lib_path) {
        Ok(lib) => lib,
        Err(msg) => {
            if let Ok(mut out) = stdout.lock() {
                let _ = write_response(&mut *out, &Response::Error { message: msg });
            }
            std::process::exit(3);
        }
    };

    if let Ok(mut out) = stdout.lock()
        && write_response(&mut *out, &Response::Ready).is_err()
    {
        return;
    }

    // Stdin reader runs on a helper thread that NEVER touches `libngspice` FFI. On `Cancel`,
    // it flips `cancelled` and wakes the main `NgspiceLib` owner thread via `BgWakeup`.
    let (work_tx, work_rx) = std::sync::mpsc::channel::<Request>();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_reader = Arc::clone(&cancelled);
    let wakeup = ng.wakeup_handle();

    let stdin_thread = std::thread::spawn(move || {
        let mut stdin = BufReader::new(io::stdin());
        let mut buf = Vec::with_capacity(64 * 1024);
        while let Ok(Some(req)) = read_request(&mut stdin, &mut buf) {
            match req {
                Request::Cancel => {
                    cancel_reader.store(true, Ordering::SeqCst);
                    wakeup.request_cancel();
                }
                Request::Shutdown => {
                    let _ = work_tx.send(Request::Shutdown);
                    break;
                }
                other => {
                    if work_tx.send(other).is_err() {
                        break;
                    }
                }
            }
        }
    });

    while let Ok(req) = work_rx.recv() {
        match req {
            Request::Ping => {
                if let Ok(mut out) = stdout.lock() {
                    let _ = write_response(&mut *out, &Response::Ready);
                }
            }
            Request::RunOp { probes, netlist } => {
                let resp = match ng.run_op(&netlist, &probes) {
                    Ok(values) => Response::OpResult { values },
                    Err(message) => Response::Error { message },
                };
                if let Ok(mut out) = stdout.lock() {
                    let _ = write_response(&mut *out, &resp);
                }
            }
            Request::RunTran {
                probes,
                netlist,
                batch_size,
            } => {
                cancelled.store(false, Ordering::SeqCst);
                let channels = match u16::try_from(probes.len()) {
                    Ok(c) => c,
                    Err(_) => {
                        if let Ok(mut out) = stdout.lock() {
                            let _ = write_response(
                                &mut *out,
                                &Response::Error {
                                    message: "probe count exceeds u16".into(),
                                },
                            );
                        }
                        continue;
                    }
                };
                let resp = match StreamingSink::new(
                    Arc::clone(&stdout),
                    channels,
                    batch_size,
                    Arc::clone(&cancelled),
                ) {
                    Ok(sink) => match ng.start_tran_bg(&netlist, &probes, Box::new(sink)) {
                        Ok(()) => match ng.wait_tran_bg_cancellable(Duration::from_secs(120)) {
                            Ok(stats) => {
                                if stats.halted_by_cancel {
                                    Response::Cancelled {
                                        total_points: stats.total_points,
                                    }
                                } else {
                                    Response::RunDone {
                                        total_points: stats.total_points,
                                    }
                                }
                            }
                            Err(message) => Response::Error { message },
                        },
                        Err(message) => Response::Error { message },
                    },
                    Err(message) => Response::Error { message },
                };
                if let Ok(mut out) = stdout.lock() {
                    let _ = write_response(&mut *out, &resp);
                }
            }
            Request::Cancel | Request::Shutdown => break,
        }
    }

    drop(ng);
    let _ = stdin_thread.join();
}
