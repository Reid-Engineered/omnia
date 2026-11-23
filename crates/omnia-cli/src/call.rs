//! `omnia call`: one request in, one response out (mcp-plan.md sections 9.1 and 14).
//!
//! The request is an envelope `{op, params, idempotency_key?}`, decoded by
//! `Request::from_slice` and answered on a new session of the service, which ends with the
//! call. The response is printed on one line, the bytes `omnia serve` prints for the same
//! request line without an `id` (AD1).

use std::io::{self, BufRead, Write};

use omnia_agent::{AgentService, Request, Response};

use crate::jsonl::{read_frame, refusal};

/// Where the request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// The command-line argument's bytes.
    Text(Vec<u8>),
    /// All of standard input (`-`).
    Stdin,
}

/// Answers the request from `source`. The response to a request that does not decode is its
/// refusal, as from the service.
pub(crate) fn response(
    service: &mut AgentService,
    source: &Source,
    stdin: &mut dyn BufRead,
) -> io::Result<Response> {
    let decoded = match source {
        Source::Text(bytes) => Request::from_slice(bytes),
        Source::Stdin => {
            let frame = read_frame(stdin, None)?;
            match frame {
                Some(frame) if frame.oversized() => Err(frame.limit_error()),
                Some(frame) => Request::from_slice(frame.bytes()),
                None => Request::from_slice(b""),
            }
        }
    };
    let session = service.open_session();
    let response = match decoded {
        Ok(request) => service.handle(session, &request),
        Err(error) => refusal(error),
    };
    service.close_session(session);
    Ok(response)
}

/// Prints `response` as one line and flushes. True if it is a success.
pub(crate) fn print(response: &Response, out: &mut dyn Write) -> io::Result<bool> {
    out.write_all(response.to_json().as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(response.outcome().is_ok())
}
