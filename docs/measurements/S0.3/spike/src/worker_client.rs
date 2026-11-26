//! Host-side client driving the isolated `omnia-ngspice-worker` subprocess over binary stdio IPC.

use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use crate::protocol::{Request, Response, read_response, write_request};

pub struct WorkerClient {
    child: Child,
    pid: u32,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    buf: Vec<u8>,
}

pub struct WorkerTranResult {
    pub total_points: u64,
    pub total_channel_samples: u64,
    pub batches: u64,
    pub first_sample_at: Option<Instant>,
    pub last_t: f64,
    pub last_ch0: f64,
}

pub struct CancelOutcome {
    pub pid: u32,
    pub cancel_elapsed: Duration,
    pub cooperative_stopped: bool,
    pub hard_killed: bool,
    pub pid_alive_after: bool,
}

impl WorkerClient {
    /// Spawn a fresh `omnia-ngspice-worker` loading `lib_path` and wait for `Ready`.
    pub fn spawn(worker_bin: &Path, lib_path: &Path) -> Result<(Self, Duration), String> {
        let t0 = Instant::now();
        let mut child = Command::new(worker_bin)
            .arg("--lib")
            .arg(lib_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", worker_bin.display()))?;
        let pid = child.id();
        let stdin = BufWriter::new(child.stdin.take().ok_or("missing worker stdin")?);
        let stdout = BufReader::with_capacity(
            64 * 1024,
            child.stdout.take().ok_or("missing worker stdout")?,
        );
        let mut client = Self {
            child,
            pid,
            stdin,
            stdout,
            buf: Vec::with_capacity(64 * 1024),
        };
        match read_response(&mut client.stdout, &mut client.buf)
            .map_err(|e| format!("failed reading Ready from worker: {e}"))?
        {
            Some(Response::Ready) => Ok((client, t0.elapsed())),
            Some(Response::Error { message }) => Err(format!("worker init error: {message}")),
            other => Err(format!("unexpected worker init response: {other:?}")),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn run_op(
        &mut self,
        netlist: Vec<String>,
        probes: Vec<String>,
    ) -> Result<Vec<f64>, String> {
        write_request(&mut self.stdin, &Request::RunOp { probes, netlist })
            .map_err(|e| format!("write RunOp failed: {e}"))?;
        match read_response(&mut self.stdout, &mut self.buf)
            .map_err(|e| format!("read OpResult failed: {e}"))?
        {
            Some(Response::OpResult { values }) => Ok(values),
            Some(Response::Error { message }) => Err(message),
            other => Err(format!("unexpected Op response: {other:?}")),
        }
    }

    pub fn run_tran(
        &mut self,
        netlist: Vec<String>,
        probes: Vec<String>,
        batch_size: u32,
    ) -> Result<WorkerTranResult, String> {
        write_request(
            &mut self.stdin,
            &Request::RunTran {
                probes,
                netlist,
                batch_size,
            },
        )
        .map_err(|e| format!("write RunTran failed: {e}"))?;

        let mut total_points = 0u64;
        let mut total_channel_samples = 0u64;
        let mut batches = 0u64;
        let mut first_sample_at = None;
        let mut last_t = 0.0;
        let mut last_ch0 = 0.0;

        loop {
            match read_response(&mut self.stdout, &mut self.buf)
                .map_err(|e| format!("read tran response failed: {e}"))?
            {
                Some(Response::SampleBatch {
                    channels,
                    points,
                    data,
                }) => {
                    if first_sample_at.is_none() && points > 0 {
                        first_sample_at = Some(Instant::now());
                    }
                    let stride = 1 + channels as usize;
                    if points > 0 && data.len() >= stride {
                        let base = (points as usize - 1) * stride;
                        last_t = data[base];
                        if channels >= 1 {
                            last_ch0 = data[base + 1];
                        }
                    }
                    total_points += points as u64;
                    total_channel_samples += points as u64 * channels as u64;
                    batches += 1;
                }
                Some(Response::RunDone { total_points: done }) => {
                    if done != total_points {
                        return Err(format!(
                            "point count mismatch: received {total_points}, worker reported {done}"
                        ));
                    }
                    return Ok(WorkerTranResult {
                        total_points,
                        total_channel_samples,
                        batches,
                        first_sample_at,
                        last_t,
                        last_ch0,
                    });
                }
                Some(Response::Error { message }) => return Err(message),
                other => return Err(format!("unexpected tran response: {other:?}")),
            }
        }
    }

    /// Start a streaming transient, wait until at least `min_points` have streamed in, then
    /// trigger `SK11` cancellation (cooperative `Cancel`/`bg_halt` with `grace` ms budget,
    /// followed by hard process kill if still running) and reap the child process (`SB4`).
    pub fn run_tran_then_cancel(
        mut self,
        netlist: Vec<String>,
        probes: Vec<String>,
        batch_size: u32,
        min_points: u64,
        grace: Duration,
        hard_kill_only: bool,
    ) -> Result<CancelOutcome, String> {
        write_request(
            &mut self.stdin,
            &Request::RunTran {
                probes,
                netlist,
                batch_size,
            },
        )
        .map_err(|e| format!("write RunTran failed: {e}"))?;

        let mut received = 0u64;
        while received < min_points {
            match read_response(&mut self.stdout, &mut self.buf)
                .map_err(|e| format!("read pre-cancel batch failed: {e}"))?
            {
                Some(Response::SampleBatch { points, .. }) => {
                    received += points as u64;
                }
                Some(Response::RunDone { .. }) => {
                    return Err("simulation finished before cancel could be triggered".into());
                }
                Some(Response::Error { message }) => return Err(message),
                other => return Err(format!("unexpected pre-cancel response: {other:?}")),
            }
        }

        let t_cancel = Instant::now();
        let mut cooperative_stopped = false;
        let mut hard_killed = false;

        if !hard_kill_only {
            let _ = write_request(&mut self.stdin, &Request::Cancel);
            // Drain stdout on a short poll until Cancelled arrives or grace expires.
            let deadline = t_cancel + grace;
            let (tx, rx) = std::sync::mpsc::channel();
            let mut stdout = self.stdout;
            let mut buf = self.buf;
            let reader = std::thread::spawn(move || {
                while let Ok(Some(resp)) = read_response(&mut stdout, &mut buf) {
                    if matches!(resp, Response::Cancelled { .. } | Response::RunDone { .. }) {
                        let _ = tx.send(true);
                        return;
                    }
                }
                let _ = tx.send(false);
            });

            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Ok(true) = rx.recv_timeout(remaining) {
                cooperative_stopped = true;
                let _ = write_request(&mut self.stdin, &Request::Shutdown);
                drop(self.stdin);
                let _ = self.child.wait();
            } else {
                hard_killed = true;
                let _ = self.child.kill();
                let _ = self.child.wait();
                drop(self.stdin);
            }
            let _ = reader.join();
        } else {
            hard_killed = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
            drop(self.stdin);
            drop(self.stdout);
        }

        let cancel_elapsed = t_cancel.elapsed();
        let pid_alive_after = is_pid_alive(self.pid);
        Ok(CancelOutcome {
            pid: self.pid,
            cancel_elapsed,
            cooperative_stopped,
            hard_killed,
            pid_alive_after,
        })
    }

    pub fn shutdown(mut self) -> Result<(), String> {
        let _ = write_request(&mut self.stdin, &Request::Shutdown);
        drop(self.stdin);
        let _ = self.child.wait();
        Ok(())
    }
}

/// Platform-specific check that a reaped worker PID is truly absent from the OS process table.
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> bool {
    // Win32 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetExitCodeProcess`.
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(
            desired_access: u32,
            inherit_handle: i32,
            process_id: u32,
        ) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}
