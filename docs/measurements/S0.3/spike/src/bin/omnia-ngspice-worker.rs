//! Prototype isolated `omnia-ngspice-worker` executable (`SS1` / `SD5`).
//!
//! Dynamically loads `libngspice`, speaks binary framed IPC on stdin/stdout, streams transient
//! `(t, ch0, ...)` double frames directly from `SendData`, and honours cooperative `Cancel`
//! (`bg_halt`) on a dedicated control thread.

use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omnia_s03_spike::ngspice::{NgspiceLib, SampleSink};
use omnia_s03_spike::protocol::{
    Request, Response, read_request, write_response, write_sample_batch_raw,
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
    ) -> Self {
        let bs = batch_size.max(1) as usize;
        let stride = 1 + channels as usize;
        Self {
            stdout,
            channels,
            batch_size: bs,
            first_flushed: false,
            f64_buf: Vec::with_capacity(bs * stride),
            wire_scratch: Vec::with_capacity(6 + bs * stride * 8),
            cancelled,
        }
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

    let ng = match NgspiceLib::load(&lib_path) {
        Ok(lib) => Arc::new(lib),
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

    // Channel dispatches work commands to a dedicated simulation thread while the main thread
    // keeps reading stdin so `Cancel` can invoke `bg_halt` immediately during `bg_run`.
    let (work_tx, work_rx) = std::sync::mpsc::channel::<Request>();
    let cancelled = Arc::new(AtomicBool::new(false));
    let ng_worker = Arc::clone(&ng);
    let out_worker = Arc::clone(&stdout);
    let cancel_worker = Arc::clone(&cancelled);

    let sim_thread = std::thread::spawn(move || {
        while let Ok(req) = work_rx.recv() {
            match req {
                Request::Ping => {
                    if let Ok(mut out) = out_worker.lock() {
                        let _ = write_response(&mut *out, &Response::Ready);
                    }
                }
                Request::RunOp { probes, netlist } => {
                    let resp = match ng_worker.run_op(&netlist, &probes) {
                        Ok(values) => Response::OpResult { values },
                        Err(message) => Response::Error { message },
                    };
                    if let Ok(mut out) = out_worker.lock() {
                        let _ = write_response(&mut *out, &resp);
                    }
                }
                Request::RunTran {
                    probes,
                    netlist,
                    batch_size,
                } => {
                    cancel_worker.store(false, Ordering::SeqCst);
                    let channels = probes.len() as u16;
                    let sink = StreamingSink::new(
                        Arc::clone(&out_worker),
                        channels,
                        batch_size,
                        Arc::clone(&cancel_worker),
                    );
                    let resp = match ng_worker.start_tran_bg(&netlist, &probes, Box::new(sink)) {
                        Ok(()) => match ng_worker.wait_tran_bg(Duration::from_secs(120)) {
                            Ok(stats) => {
                                if cancel_worker.load(Ordering::SeqCst) {
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
                    };
                    if let Ok(mut out) = out_worker.lock() {
                        let _ = write_response(&mut *out, &resp);
                    }
                }
                Request::Cancel | Request::Shutdown => break,
            }
        }
    });

    let mut stdin = BufReader::new(io::stdin());
    let mut buf = Vec::with_capacity(64 * 1024);
    while let Ok(Some(req)) = read_request(&mut stdin, &mut buf) {
        match req {
            Request::Cancel => {
                cancelled.store(true, Ordering::SeqCst);
                let _ = ng.bg_halt();
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
    drop(work_tx);
    let _ = sim_thread.join();
}
