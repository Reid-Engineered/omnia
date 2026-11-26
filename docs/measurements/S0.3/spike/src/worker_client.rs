//! Host-side client driving the isolated `omnia-ngspice-worker` subprocess over binary stdio IPC.

use std::io::{self, BufReader, BufWriter};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelMode {
    /// Worker acknowledged `Response::Cancelled`, exited cleanly (`status.success()`), and was
    /// reaped at or before `grace`.
    CooperativeReaped,
    /// Worker acknowledged `Response::Cancelled` and exited cleanly (`status.success()`) on its
    /// own after `grace` expired, without the host invoking `kill()`.
    CooperativeLateReaped,
    /// Host invoked `kill()` + `wait()` on a still-running worker (either `hard_kill_only` or
    /// because the worker was still alive when the `grace` deadline expired).
    HardKilled,
}

fn classify_cooperative_exit(
    success: bool,
    within_deadline: bool,
    status_desc: impl std::fmt::Display,
) -> Result<CancelMode, String> {
    if !success {
        return Err(format!(
            "worker exited with non-zero status during cooperative cancel: {status_desc}"
        ));
    }
    if within_deadline {
        Ok(CancelMode::CooperativeReaped)
    } else {
        Ok(CancelMode::CooperativeLateReaped)
    }
}

#[derive(Debug)]
pub struct CancelOutcome {
    pub pid: u32,
    pub cancel_elapsed: Duration,
    pub mode: CancelMode,
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
    /// trigger `SK11` cancellation:
    /// - The `grace` deadline (50 ms) covers the ENTIRE cooperative cancel + worker clean exit
    ///   + `try_wait`/reap path.
    /// - If `Response::RunDone` arrives before `Response::Cancelled`, the simulation finished
    ///   naturally before halt took effect — returned as an explicit error and never counted as
    ///   a cooperative stop.
    /// - If the worker has not exited and been reaped before `deadline`, the host immediately
    ///   hard-kills and reaps it and records `CancelMode::HardKilled`.
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
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err("simulation finished before cancel could be triggered".into());
                }
                Some(Response::Error { message }) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(message);
                }
                other => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(format!("unexpected pre-cancel response: {other:?}"));
                }
            }
        }

        enum StopSignal {
            Cancelled,
            NaturalRunDone,
            Error(String),
            Eof,
        }

        let t_cancel = Instant::now();
        let deadline = t_cancel + grace;
        let mode: CancelMode;

        if !hard_kill_only {
            let _ = write_request(&mut self.stdin, &Request::Cancel);
            let (tx, rx) = std::sync::mpsc::channel();
            let mut stdout = self.stdout;
            let mut buf = self.buf;
            let reader = std::thread::spawn(move || {
                loop {
                    match read_response(&mut stdout, &mut buf) {
                        Ok(Some(Response::SampleBatch { .. })) => continue,
                        Ok(Some(Response::Cancelled { .. })) => {
                            let _ = tx.send(StopSignal::Cancelled);
                            return;
                        }
                        Ok(Some(Response::RunDone { .. })) => {
                            let _ = tx.send(StopSignal::NaturalRunDone);
                            return;
                        }
                        Ok(Some(Response::Error { message })) => {
                            let _ = tx.send(StopSignal::Error(message));
                            return;
                        }
                        Ok(_) | Err(_) => {
                            let _ = tx.send(StopSignal::Eof);
                            return;
                        }
                    }
                }
            });

            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut reaped_status = None;
            match rx.recv_timeout(remaining) {
                Ok(StopSignal::Cancelled) => {
                    let _ = write_request(&mut self.stdin, &Request::Shutdown);
                    drop(self.stdin);
                    // Enforce the SAME `deadline` over clean worker exit + reap.
                    loop {
                        if Instant::now() >= deadline {
                            break;
                        }
                        match self.child.try_wait() {
                            Ok(Some(status)) => {
                                reaped_status = Some((status, Instant::now() <= deadline));
                                break;
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                            Err(e) => {
                                let _ = self.child.kill();
                                let _ = self.child.wait();
                                let _ = reader.join();
                                return Err(format!(
                                    "try_wait failed during cooperative cancel shutdown: {e}"
                                ));
                            }
                        }
                    }

                    if let Some((status, within_deadline)) = reaped_status {
                        let _ = reader.join();
                        mode =
                            classify_cooperative_exit(status.success(), within_deadline, status)?;
                    } else {
                        match self.child.try_wait() {
                            Ok(Some(status)) => {
                                let _ = reader.join();
                                mode = classify_cooperative_exit(status.success(), false, status)?;
                            }
                            Ok(None) => {
                                let _ = self.child.kill();
                                let _ = self.child.wait();
                                let _ = reader.join();
                                mode = CancelMode::HardKilled;
                            }
                            Err(e) => {
                                let _ = self.child.kill();
                                let _ = self.child.wait();
                                let _ = reader.join();
                                return Err(format!("final try_wait failed before hard kill: {e}"));
                            }
                        }
                    }
                }
                Ok(StopSignal::NaturalRunDone) => {
                    drop(self.stdin);
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let _ = reader.join();
                    return Err(
                        "simulation completed naturally (RunDone) before Cancel took effect".into(),
                    );
                }
                Ok(StopSignal::Error(msg)) => {
                    drop(self.stdin);
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let _ = reader.join();
                    return Err(format!("worker error during cancel: {msg}"));
                }
                Ok(StopSignal::Eof) => {
                    drop(self.stdin);
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let _ = reader.join();
                    return Err(
                        "worker closed stdout unexpectedly before acknowledging Cancel".into(),
                    );
                }
                Err(_) => {
                    drop(self.stdin);
                    match self.child.try_wait() {
                        Ok(Some(status)) => {
                            let _ = reader.join();
                            return Err(format!(
                                "worker exited without acknowledging Cancel (status={status})"
                            ));
                        }
                        Ok(None) => {
                            let _ = self.child.kill();
                            let _ = self.child.wait();
                            let _ = reader.join();
                            mode = CancelMode::HardKilled;
                        }
                        Err(e) => {
                            let _ = self.child.kill();
                            let _ = self.child.wait();
                            let _ = reader.join();
                            return Err(format!("try_wait failed after cancel timeout: {e}"));
                        }
                    }
                }
            }
        } else {
            mode = CancelMode::HardKilled;
            let _ = self.child.kill();
            let _ = self.child.wait();
            drop(self.stdin);
            drop(self.stdout);
        }

        let cancel_elapsed = t_cancel.elapsed();
        let pid_alive_after = is_pid_alive(self.pid)
            .map_err(|e| format!("OS PID liveness query failed for pid {}: {e}", self.pid))?;
        Ok(CancelOutcome {
            pid: self.pid,
            cancel_elapsed,
            mode,
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

/// Platform-specific check that a reaped worker PID is absent from the OS process table.
/// Returns `Err` on unexpected OS query errors rather than silently assuming the process is dead.
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> io::Result<bool> {
    match std::fs::metadata(format!("/proc/{pid}")) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> io::Result<bool> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: i32 = 87;

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
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                return Ok(false);
            }
            return Err(err);
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        let err = if ok == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        CloseHandle(handle);
        if let Some(e) = err {
            return Err(e);
        }
        Ok(code == STILL_ACTIVE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooperative_exit_classification_requires_success_and_distinguishes_late_reap() {
        assert_eq!(
            classify_cooperative_exit(true, true, "0"),
            Ok(CancelMode::CooperativeReaped)
        );
        assert_eq!(
            classify_cooperative_exit(true, false, "0"),
            Ok(CancelMode::CooperativeLateReaped)
        );
        assert!(classify_cooperative_exit(false, true, "1").is_err());
        assert!(classify_cooperative_exit(false, false, "1").is_err());
    }
}
