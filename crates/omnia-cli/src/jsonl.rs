//! `omnia serve`: a JSON-lines session on stdin and stdout (mcp-plan.md sections 9.1 and 14).
//!
//! One session per stream (section 11.1): documents, idempotency keys and approvals live until
//! the input ends. Each non-blank input line is one request envelope, optionally with an `"id"`
//! (a string or an integer) that the response line echoes as its first field. Without an `id`
//! the response line is exactly the bytes `omnia call` prints for the same request (AD1).
//!
//! A line is decoded in the order of `Request::from_slice`: AK2 on the raw line, strict
//! parsing (duplicate keys and number spelling are refused, so the `id` is lifted from a value
//! that still had them checked), then the envelope decoder on the rest. A line that cannot be
//! parsed, or whose `id` is neither a string nor an integer, is answered without an `id`. A
//! blank line is skipped. A bad line never ends the session.

use std::io::{self, BufRead, ErrorKind as IoErrorKind, Write};

use omnia_agent::{
    AgentError, AgentService, ErrorKind, MAX_REQUEST_BYTES, Request, Response, SessionId,
    error::Limit, schema::parse_strict,
};
use serde_json::Value;

/// One frame of input: a line without its terminator, or the whole input for `omnia call -`.
///
/// At most `MAX_REQUEST_BYTES + 1` bytes are kept, so an oversized frame never sits in memory
/// whole; `len` is the frame's full length, which an AK2 refusal reports.
pub(crate) struct Frame {
    bytes: Vec<u8>,
    len: u64,
    /// Whether the frame's last byte is a carriage return.
    last_cr: bool,
}

impl Frame {
    /// The frame's bytes; complete unless [`Self::oversized`].
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the frame is over AK2 and so was not kept whole.
    pub(crate) fn oversized(&self) -> bool {
        self.len > MAX_REQUEST_BYTES
    }

    /// The AK2 refusal for this frame, the error `Request::from_slice` gives for the same
    /// bytes.
    pub(crate) fn limit_error(&self) -> AgentError {
        AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::RequestBytes,
            value: self.len,
        })
    }

    /// Drops one trailing carriage return, so `\r\n` line ends read like `\n`.
    fn without_cr(mut self) -> Self {
        if self.last_cr {
            self.last_cr = false;
            self.len -= 1;
            // Kept whole, the frame still holds the carriage return; cut short, it never did.
            if let Ok(len) = usize::try_from(self.len)
                && self.bytes.len() > len
            {
                self.bytes.truncate(len);
            }
        }
        self
    }
}

/// Bytes kept of a frame: one past AK2, so a frame of exactly AK2 bytes plus a carriage
/// return is still kept whole.
const KEEP: u64 = MAX_REQUEST_BYTES + 1;

/// Reads one frame: up to `delim` (consumed, not included) or to the end of the input.
/// `None` at the end of the input when nothing was read.
pub(crate) fn read_frame(reader: &mut dyn BufRead, delim: Option<u8>) -> io::Result<Option<Frame>> {
    let mut frame = Frame {
        bytes: Vec::new(),
        len: 0,
        last_cr: false,
    };
    let mut any = false;
    loop {
        let buf = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == IoErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return Ok(any.then_some(frame));
        }
        any = true;
        let end = delim.and_then(|d| buf.iter().position(|&b| b == d));
        let chunk = &buf[..end.unwrap_or(buf.len())];
        let room = usize::try_from(KEEP.saturating_sub(frame.bytes.len() as u64))
            .unwrap_or(usize::MAX)
            .min(chunk.len());
        frame.bytes.extend_from_slice(&chunk[..room]);
        if let Some(&last) = chunk.last() {
            frame.last_cr = last == b'\r';
        }
        frame.len = frame.len.saturating_add(chunk.len() as u64);
        let consumed = end.map_or(buf.len(), |i| i + 1);
        reader.consume(consumed);
        if end.is_some() {
            return Ok(Some(frame));
        }
    }
}

/// A failure that belongs to no document: a request that did not decode.
pub(crate) fn refusal(error: AgentError) -> Response {
    Response::new(None, None, Err(error)).expect("an error response needs no document")
}

/// Serves `input` line by line on one new session of `service`, writing one response line per
/// request to `out` and flushing after each. Returns at the end of the input; the session is
/// closed either way.
pub(crate) fn serve(
    service: &mut AgentService,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> io::Result<()> {
    let session = service.open_session();
    let result = serve_session(service, session, input, out);
    service.close_session(session);
    result
}

fn serve_session(
    service: &mut AgentService,
    session: SessionId,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> io::Result<()> {
    while let Some(frame) = read_frame(input, Some(b'\n'))? {
        let frame = frame.without_cr();
        if !frame.oversized() && frame.bytes().iter().all(|b| is_json_space(*b)) {
            continue;
        }
        let line = answer(service, session, &frame);
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}

/// JSON's insignificant whitespace (RFC 8259 section 2).
fn is_json_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// The response line for one input line.
pub(crate) fn answer(service: &mut AgentService, session: SessionId, frame: &Frame) -> String {
    if frame.oversized() {
        return refusal(frame.limit_error()).to_json();
    }
    let value = match parse_strict(frame.bytes()) {
        Ok(value) => value,
        Err(error) => return refusal(error).to_json(),
    };
    let (id, envelope) = match lift_id(value) {
        Ok(parts) => parts,
        Err(error) => return refusal(error).to_json(),
    };
    let response = match Request::from_envelope_value(envelope) {
        Ok(request) => service.handle(session, &request),
        Err(error) => refusal(error),
    };
    echo(id.as_ref(), &response.to_json())
}

/// Takes the JSON-lines `id` off an envelope parsed by `parse_strict`. Anything that is not an
/// object is left for the envelope decoder to refuse.
fn lift_id(value: Value) -> Result<(Option<Value>, Value), AgentError> {
    let Value::Object(mut map) = value else {
        return Ok((None, value));
    };
    let id = match map.remove("id") {
        None => None,
        Some(id @ Value::String(_)) => Some(id),
        Some(id @ Value::Number(_)) if id.is_i64() || id.is_u64() => Some(id),
        Some(_) => {
            return Err(AgentError::with_message(
                ErrorKind::SchemaViolation {
                    path: "/id".to_owned(),
                },
                "expected string or integer",
            ));
        }
    };
    Ok((id, Value::Object(map)))
}

/// The response line: `response` itself, or with `"id"` as its first field.
fn echo(id: Option<&Value>, response: &str) -> String {
    match id {
        None => response.to_owned(),
        Some(id) => {
            let rest = response
                .strip_prefix('{')
                .expect("a response is a JSON object");
            let id = serde_json::to_string(id).expect("ids serialize");
            format!("{{\"id\":{id},{rest}")
        }
    }
}
