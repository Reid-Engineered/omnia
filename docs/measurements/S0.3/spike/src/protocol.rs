//! Binary framed IPC protocol over stdio between host and `omnia-ngspice-worker`.
//!
//! Frame layout (all integers and `f64` little-endian):
//! `[tag: u8][payload_len: u32][payload: payload_len bytes]`
//!
//! Payload cap (`MAX_FRAME_BYTES`) prevents unbounded allocation on corrupt frames.

use std::io::{self, Read, Write};

pub const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

pub const TAG_PING: u8 = 0x01;
pub const TAG_RUN_OP: u8 = 0x02;
pub const TAG_RUN_TRAN: u8 = 0x03;
pub const TAG_CANCEL: u8 = 0x04;
pub const TAG_SHUTDOWN: u8 = 0x05;

pub const TAG_READY: u8 = 0x81;
pub const TAG_OP_RESULT: u8 = 0x82;
pub const TAG_SAMPLE_BATCH: u8 = 0x83;
pub const TAG_RUN_DONE: u8 = 0x84;
pub const TAG_CANCELLED: u8 = 0x85;
pub const TAG_ERROR: u8 = 0x86;

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Ping,
    RunOp {
        probes: Vec<String>,
        netlist: Vec<String>,
    },
    RunTran {
        probes: Vec<String>,
        netlist: Vec<String>,
        batch_size: u32,
    },
    Cancel,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Ready,
    OpResult {
        values: Vec<f64>,
    },
    /// Each point has `1 + channels` `f64` values: `[t, ch0, ch1, ...]`.
    SampleBatch {
        channels: u16,
        points: u32,
        data: Vec<f64>,
    },
    RunDone {
        total_points: u64,
    },
    Cancelled {
        total_points: u64,
    },
    Error {
        message: String,
    },
}

fn write_raw_frame(w: &mut impl Write, tag: u8, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame exceeds MAX_FRAME_BYTES",
        ));
    }
    let mut hdr = [0u8; 5];
    hdr[0] = tag;
    hdr[1..5].copy_from_slice(&len.to_le_bytes());
    w.write_all(&hdr)?;
    if !payload.is_empty() {
        w.write_all(payload)?;
    }
    Ok(())
}

fn read_raw_frame(r: &mut impl Read, buf: &mut Vec<u8>) -> io::Result<Option<u8>> {
    let mut hdr = [0u8; 5];
    match r.read_exact(&mut hdr) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let tag = hdr[0];
    let len = u32::from_le_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]);
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds limit {MAX_FRAME_BYTES}"),
        ));
    }
    buf.resize(len as usize, 0);
    if len > 0 {
        r.read_exact(buf)?;
    }
    Ok(Some(tag))
}

fn encode_str_list(out: &mut Vec<u8>, items: &[String]) {
    out.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for s in items {
        let b = s.as_bytes();
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(b);
    }
}

fn decode_str_list(input: &[u8], pos: &mut usize) -> io::Result<Vec<String>> {
    let count = read_u32(input, pos)? as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let len = read_u32(input, pos)? as usize;
        if *pos + len > input.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated string in frame",
            ));
        }
        let s = std::str::from_utf8(&input[*pos..*pos + len])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        *pos += len;
        out.push(s.to_owned());
    }
    Ok(out)
}

fn read_u16(input: &[u8], pos: &mut usize) -> io::Result<u16> {
    if *pos + 2 > input.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated u16"));
    }
    let v = u16::from_le_bytes([input[*pos], input[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_u32(input: &[u8], pos: &mut usize) -> io::Result<u32> {
    if *pos + 4 > input.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated u32"));
    }
    let v = u32::from_le_bytes([
        input[*pos],
        input[*pos + 1],
        input[*pos + 2],
        input[*pos + 3],
    ]);
    *pos += 4;
    Ok(v)
}

fn read_u64(input: &[u8], pos: &mut usize) -> io::Result<u64> {
    if *pos + 8 > input.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated u64"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_le_bytes(bytes))
}

fn read_f64(input: &[u8], pos: &mut usize) -> io::Result<f64> {
    if *pos + 8 > input.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated f64"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[*pos..*pos + 8]);
    *pos += 8;
    Ok(f64::from_le_bytes(bytes))
}

pub fn write_request(w: &mut impl Write, req: &Request) -> io::Result<()> {
    let mut payload = Vec::new();
    match req {
        Request::Ping => write_raw_frame(w, TAG_PING, &[])?,
        Request::RunOp { probes, netlist } => {
            encode_str_list(&mut payload, probes);
            encode_str_list(&mut payload, netlist);
            write_raw_frame(w, TAG_RUN_OP, &payload)?;
        }
        Request::RunTran {
            probes,
            netlist,
            batch_size,
        } => {
            payload.extend_from_slice(&batch_size.to_le_bytes());
            encode_str_list(&mut payload, probes);
            encode_str_list(&mut payload, netlist);
            write_raw_frame(w, TAG_RUN_TRAN, &payload)?;
        }
        Request::Cancel => write_raw_frame(w, TAG_CANCEL, &[])?,
        Request::Shutdown => write_raw_frame(w, TAG_SHUTDOWN, &[])?,
    }
    w.flush()
}

pub fn read_request(r: &mut impl Read, buf: &mut Vec<u8>) -> io::Result<Option<Request>> {
    let Some(tag) = read_raw_frame(r, buf)? else {
        return Ok(None);
    };
    let mut pos = 0;
    let req = match tag {
        TAG_PING => Request::Ping,
        TAG_RUN_OP => {
            let probes = decode_str_list(buf, &mut pos)?;
            let netlist = decode_str_list(buf, &mut pos)?;
            Request::RunOp { probes, netlist }
        }
        TAG_RUN_TRAN => {
            let batch_size = read_u32(buf, &mut pos)?;
            let probes = decode_str_list(buf, &mut pos)?;
            let netlist = decode_str_list(buf, &mut pos)?;
            Request::RunTran {
                probes,
                netlist,
                batch_size,
            }
        }
        TAG_CANCEL => Request::Cancel,
        TAG_SHUTDOWN => Request::Shutdown,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown request tag 0x{other:02x}"),
            ));
        }
    };
    Ok(Some(req))
}

/// Zero-allocation helper to stream a `SampleBatch` directly from a caller-owned byte buffer.
pub fn write_sample_batch_raw(
    w: &mut impl Write,
    channels: u16,
    points: u32,
    samples_f64: &[f64],
    scratch: &mut Vec<u8>,
) -> io::Result<()> {
    let expected = points as usize * (1 + channels as usize);
    debug_assert_eq!(samples_f64.len(), expected);
    scratch.clear();
    scratch.reserve(6 + samples_f64.len() * 8);
    scratch.extend_from_slice(&channels.to_le_bytes());
    scratch.extend_from_slice(&points.to_le_bytes());
    for &v in samples_f64 {
        scratch.extend_from_slice(&v.to_le_bytes());
    }
    write_raw_frame(w, TAG_SAMPLE_BATCH, scratch)
}

pub fn write_response(w: &mut impl Write, resp: &Response) -> io::Result<()> {
    let mut payload = Vec::new();
    match resp {
        Response::Ready => write_raw_frame(w, TAG_READY, &[])?,
        Response::OpResult { values } => {
            payload.extend_from_slice(&(values.len() as u32).to_le_bytes());
            for &v in values {
                payload.extend_from_slice(&v.to_le_bytes());
            }
            write_raw_frame(w, TAG_OP_RESULT, &payload)?;
        }
        Response::SampleBatch {
            channels,
            points,
            data,
        } => {
            write_sample_batch_raw(w, *channels, *points, data, &mut payload)?;
        }
        Response::RunDone { total_points } => {
            write_raw_frame(w, TAG_RUN_DONE, &total_points.to_le_bytes())?;
        }
        Response::Cancelled { total_points } => {
            write_raw_frame(w, TAG_CANCELLED, &total_points.to_le_bytes())?;
        }
        Response::Error { message } => {
            write_raw_frame(w, TAG_ERROR, message.as_bytes())?;
        }
    }
    w.flush()
}

pub fn read_response(r: &mut impl Read, buf: &mut Vec<u8>) -> io::Result<Option<Response>> {
    let Some(tag) = read_raw_frame(r, buf)? else {
        return Ok(None);
    };
    let mut pos = 0;
    let resp = match tag {
        TAG_READY => Response::Ready,
        TAG_OP_RESULT => {
            let count = read_u32(buf, &mut pos)? as usize;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(read_f64(buf, &mut pos)?);
            }
            Response::OpResult { values }
        }
        TAG_SAMPLE_BATCH => {
            let channels = read_u16(buf, &mut pos)?;
            let points = read_u32(buf, &mut pos)?;
            let expected = points as usize * (1 + channels as usize);
            let mut data = Vec::with_capacity(expected);
            for _ in 0..expected {
                data.push(read_f64(buf, &mut pos)?);
            }
            Response::SampleBatch {
                channels,
                points,
                data,
            }
        }
        TAG_RUN_DONE => Response::RunDone {
            total_points: read_u64(buf, &mut pos)?,
        },
        TAG_CANCELLED => Response::Cancelled {
            total_points: read_u64(buf, &mut pos)?,
        },
        TAG_ERROR => {
            let message = String::from_utf8_lossy(buf).into_owned();
            Response::Error { message }
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown response tag 0x{other:02x}"),
            ));
        }
    };
    Ok(Some(resp))
}
