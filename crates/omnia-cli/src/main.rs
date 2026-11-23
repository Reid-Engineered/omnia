//! The `omnia` binary: transports over `omnia_agent::AgentService` (mcp-plan.md sections 5 and
//! 14). It parses arguments, reads and prints; every request is answered by the service (AD1),
//! and nothing here holds domain logic.
//!
//! - `omnia capabilities` prints the catalog: the `result` of a `capabilities` request.
//! - `omnia schema [NAME]` prints the published JSON Schema `NAME` (`request`, `response`,
//!   `<op>.params`, `<op>.arguments`, `<op>.response`; a `.json` suffix is accepted), the bytes
//!   committed under `crates/omnia-agent/schema/`. Without a name it lists the names.
//! - `omnia call [--root DIR]... REQUEST` answers one envelope, given as text or as `-` for
//!   standard input (`call.rs`).
//! - `omnia serve [--root DIR]...` is a JSON-lines session on standard input and output
//!   (`jsonl.rs`).
//!
//! Exit codes (section 14): 0 success; 1 a tool error (the response on stdout is an error), or
//! an I/O failure reading the input or writing the output; 2 a usage error. `serve` exits 0 at
//! the end of its input. Standard output carries only results; usage errors and diagnostics go
//! to standard error.

mod call;
mod jsonl;

use std::{
    ffi::{OsStr, OsString},
    io::{self, BufRead, Write},
    path::PathBuf,
    process::ExitCode,
};

use omnia_agent::{
    AgentService, Operation, Request, Roots,
    schema::{NoArgs, schema_files, schema_text},
};

/// Success.
const EXIT_OK: u8 = 0;
/// A tool error, or an I/O failure on the standard streams.
const EXIT_FAILURE: u8 = 1;
/// A usage error: bad arguments or a root that cannot be used.
const EXIT_USAGE: u8 = 2;

const USAGE: &str = "\
usage: omnia <command>

commands:
  capabilities                    print the capability catalog
  schema [NAME]                   print the JSON Schema NAME, or list the names
  call [--root DIR]... [--] REQUEST
                                  answer one request envelope; REQUEST is JSON text, or -
                                  to read it from standard input; put -- before a REQUEST
                                  that starts with -
  serve [--root DIR]...           JSON-lines session on standard input and output
  help                            print this text

exit codes: 0 success, 1 tool error, 2 usage error
";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = run(
        &args,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(code)
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Capabilities,
    Schema(Option<String>),
    Call {
        roots: Vec<PathBuf>,
        source: call::Source,
    },
    Serve {
        roots: Vec<PathBuf>,
    },
}

/// Runs the command line `args` (without the program name) on the given streams and returns
/// the exit code.
fn run(
    args: &[OsString],
    stdin: &mut dyn BufRead,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let command = match parse(args) {
        Ok(command) => command,
        Err(message) => return usage_error(stderr, &message),
    };
    let outcome = match command {
        Command::Help => stdout.write_all(USAGE.as_bytes()).map(|()| EXIT_OK),
        Command::Capabilities => capabilities(stdout),
        Command::Schema(name) => match name {
            None => list_schemas(stdout),
            Some(name) => match schema_text(name.strip_suffix(".json").unwrap_or(&name)) {
                Some(text) => stdout.write_all(text.as_bytes()).map(|()| EXIT_OK),
                None => {
                    let message =
                        format!("unknown schema `{name}`; `omnia schema` lists the names");
                    return usage_error(stderr, &message);
                }
            },
        },
        Command::Call { roots, source } => match service(&roots) {
            Ok(mut service) => call::response(&mut service, &source, stdin)
                .and_then(|response| call::print(&response, stdout))
                .map(|ok| if ok { EXIT_OK } else { EXIT_FAILURE }),
            Err(message) => return usage_error(stderr, &message),
        },
        Command::Serve { roots } => match service(&roots) {
            Ok(mut service) => jsonl::serve(&mut service, stdin, stdout).map(|()| EXIT_OK),
            Err(message) => return usage_error(stderr, &message),
        },
    };
    let outcome = outcome.and_then(|code| stdout.flush().map(|()| code));
    match outcome {
        Ok(code) => code,
        Err(error) => {
            // Standard error is the last channel left; a failure there has nowhere to go.
            let _ = writeln!(stderr, "omnia: {error}");
            EXIT_FAILURE
        }
    }
}

fn usage_error(stderr: &mut dyn Write, message: &str) -> u8 {
    // Standard error is the last channel left; a failure there has nowhere to go.
    let _ = write!(stderr, "omnia: {message}\n\n{USAGE}");
    EXIT_USAGE
}

/// Parses the arguments after the program name.
fn parse(args: &[OsString]) -> Result<Command, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err("no command".to_owned());
    };
    let command = command.to_str().unwrap_or("");
    match command {
        "help" | "--help" | "-h" => no_more(rest, Command::Help),
        "capabilities" => no_more(rest, Command::Capabilities),
        "schema" => match rest {
            [] => Ok(Command::Schema(None)),
            [name] => name
                .to_str()
                .map(|n| Command::Schema(Some(n.to_owned())))
                .ok_or_else(|| "schema names are UTF-8".to_owned()),
            _ => Err("`schema` takes at most one name".to_owned()),
        },
        "call" => {
            let (roots, positional) = options(rest)?;
            match positional.as_slice() {
                [request] if *request == "-" => Ok(Command::Call {
                    roots,
                    source: call::Source::Stdin,
                }),
                [request] => Ok(Command::Call {
                    roots,
                    source: call::Source::Text(request.as_encoded_bytes().to_vec()),
                }),
                [] => Err("`call` needs a request".to_owned()),
                _ => Err("`call` takes one request".to_owned()),
            }
        }
        "serve" => {
            let (roots, positional) = options(rest)?;
            if positional.is_empty() {
                Ok(Command::Serve { roots })
            } else {
                Err("`serve` takes no request; it reads them from standard input".to_owned())
            }
        }
        _ => Err(format!(
            "unknown command `{}`",
            args[0].to_string_lossy().into_owned()
        )),
    }
}

fn no_more(rest: &[OsString], command: Command) -> Result<Command, String> {
    if rest.is_empty() {
        Ok(command)
    } else {
        Err(format!(
            "unexpected argument `{}`",
            rest[0].to_string_lossy()
        ))
    }
}

/// Splits `--root DIR` and `--root=DIR` (repeatable) from the positional arguments. `--` ends
/// the options; `-` is positional (standard input).
fn options(args: &[OsString]) -> Result<(Vec<PathBuf>, Vec<&OsStr>), String> {
    let mut roots = Vec::new();
    let mut positional = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let bytes = arg.as_encoded_bytes();
        if bytes == b"--" {
            positional.extend(iter.by_ref().map(OsString::as_os_str));
        } else if bytes == b"--root" {
            let dir = iter.next().ok_or("`--root` needs a directory")?;
            roots.push(PathBuf::from(dir));
        } else if let Some(dir) = arg.to_str().and_then(|a| a.strip_prefix("--root=")) {
            roots.push(PathBuf::from(dir));
        } else if bytes.starts_with(b"-") && bytes != b"-" {
            return Err(format!("unknown option `{}`", arg.to_string_lossy()));
        } else {
            positional.push(arg.as_os_str());
        }
    }
    Ok((roots, positional))
}

/// A headless service over `roots` under the AQ4 policy (section 12.1). A root that does not
/// exist or is not a directory is a usage error naming it.
fn service(roots: &[PathBuf]) -> Result<AgentService, String> {
    for root in roots {
        Roots::new([root]).map_err(|e| format!("--root {}: {}", root.display(), e.message))?;
    }
    let roots = Roots::new(roots).map_err(|e| e.message)?;
    Ok(AgentService::headless_with_roots(roots))
}

/// Prints the catalog: the `result` of a `capabilities` request, answered by the service.
fn capabilities(stdout: &mut dyn Write) -> io::Result<u8> {
    let mut service = AgentService::headless();
    let session = service.open_session();
    let request = Request::new(Operation::Capabilities(NoArgs {}), None)
        .expect("the capabilities request is valid");
    let response = service.handle(session, &request);
    let catalog = match response.outcome() {
        Ok(output) => serde_json::to_string(output).expect("results serialize"),
        Err(error) => {
            return Err(io::Error::other(format!(
                "capabilities failed: {}",
                error.message
            )));
        }
    };
    stdout.write_all(catalog.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(EXIT_OK)
}

/// Lists the published schema names, one per line, sorted.
fn list_schemas(stdout: &mut dyn Write) -> io::Result<u8> {
    for (name, _) in schema_files() {
        writeln!(stdout, "{name}")?;
    }
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    //! A1.7 done-when: transcript tests run the command line in-process on byte streams and
    //! show that the same request gives byte-identical results through `call` and `serve`
    //! (AD1), that `serve` adds nothing but the echoed `id`, and that standard output carries
    //! only responses (usage errors and diagnostics go to standard error).

    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use omnia_agent::{MAX_REQUEST_BYTES, OpName, Response, schema::parse_strict};
    use serde_json::{Value, json};

    use super::*;

    const RESISTOR: &str = "Resistor-IEEE-Standard";
    /// One K2 pin-lattice step in nanometres.
    const STEP: i64 = 1_270_000;

    /// A scratch directory used as `--root`, removed on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("omnia-cli-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create the scratch root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Run {
        code: u8,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    impl Run {
        fn text(&self) -> &str {
            std::str::from_utf8(&self.stdout).expect("stdout is UTF-8")
        }
    }

    fn omnia(args: &[OsString], stdin: &[u8]) -> Run {
        let mut input = stdin;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run(args, &mut input, &mut stdout, &mut stderr);
        Run {
            code,
            stdout,
            stderr,
        }
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn rooted(command: &str, root: &Path) -> Vec<OsString> {
        vec![
            OsString::from(command),
            OsString::from("--root"),
            root.as_os_str().to_owned(),
        ]
    }

    /// `omnia call --root ROOT REQUEST`.
    fn call_arg(root: &Path, request: &str) -> Run {
        let mut a = rooted("call", root);
        a.push(OsString::from(request));
        omnia(&a, b"")
    }

    /// `omnia call --root ROOT -` with the request on standard input.
    fn call_stdin(root: &Path, request: &[u8]) -> Run {
        let mut a = rooted("call", root);
        a.push(OsString::from("-"));
        omnia(&a, request)
    }

    /// `omnia serve --root ROOT` with `input` on standard input.
    fn serve(root: &Path, input: &[u8]) -> Run {
        omnia(&rooted("serve", root), input)
    }

    /// The JSON-lines framing: `envelope` with `"id"` as its first field.
    fn with_id(id: &str, envelope: &str) -> String {
        let rest = envelope.trim_start().strip_prefix('{').expect("an object");
        format!("{{\"id\":{id},{rest}")
    }

    fn pt(x: i64, y: i64) -> Value {
        json!([x * STEP, y * STEP])
    }

    fn place(x: i64, designator: &str) -> Value {
        json!({"op": "place_part", "symbol": RESISTOR, "at": pt(x, 0), "orient": "identity",
               "designator": designator, "value": "10k"})
    }

    fn wire() -> Value {
        json!({"op": "add_wires", "wires": [[pt(4, 0), pt(8, 0)]]})
    }

    fn field(response: &str, pointer: &str) -> Value {
        let value: Value = serde_json::from_str(response).expect("JSON");
        value
            .pointer(pointer)
            .cloned()
            .unwrap_or_else(|| panic!("no {pointer} in {response}"))
    }

    /// Writes `fixture.ron` (two resistors, saved at revision 1) into `root` through `serve`.
    fn write_fixture(root: &Path) {
        let lines = [
            json!({"op": "new", "params": {}}),
            json!({"op": "apply", "params": {"doc_id": "d1", "expected_revision": 0,
                   "operations": [place(0, "R1"), place(12, "R2")]}}),
            json!({"op": "save", "params": {"doc_id": "d1", "path": "fixture.ron"}}),
        ];
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let out = serve(root, input.as_bytes());
        assert_eq!(out.code, EXIT_OK);
        assert!(
            out.text().lines().all(|l| l.contains("\"result\"")),
            "fixture: {}",
            out.text()
        );
        assert!(root.join("fixture.ron").is_file());
    }

    /// One request of the corpus, the operation whose response schema its answer must meet,
    /// and whether an `id` added to its line is echoed (the line still parses as an object
    /// within AK2).
    struct Case {
        name: &'static str,
        request: String,
        op: OpName,
        echoes_id: bool,
    }

    fn case(name: &'static str, request: &str, op: OpName, echoes_id: bool) -> Case {
        Case {
            name,
            request: request.to_owned(),
            op,
            echoes_id,
        }
    }

    /// Successes and refusals of every kind a single request can meet: each step of section
    /// 9.1's order of checks, resolution, files (ATC-path-01) and concurrency.
    fn corpus() -> Vec<Case> {
        let padded = |len: u64| {
            let base = r#"{"op":"new","params":{}}"#;
            let pad = usize::try_from(len).unwrap() - base.len();
            format!("{base}{}", " ".repeat(pad))
        };
        let new = OpName::New;
        vec![
            case(
                "capabilities",
                r#"{"op":"capabilities","params":{}}"#,
                OpName::Capabilities,
                true,
            ),
            // AINV5: keys in another order give the same bytes.
            case(
                "keys reversed",
                r#"{"params":{},"op":"capabilities"}"#,
                OpName::Capabilities,
                true,
            ),
            case("new", r#"{"op":"new","params":{}}"#, new, true),
            case(
                "new, spaced",
                "  {\"op\" : \"new\", \"params\" : { } }\t ",
                new,
                true,
            ),
            case(
                "open inside the root",
                r#"{"op":"open","params":{"path":"fixture.ron"}}"#,
                OpName::Open,
                true,
            ),
            case(
                "ATC-path-01 ..",
                r#"{"op":"open","params":{"path":"../fixture.ron"}}"#,
                OpName::Open,
                true,
            ),
            case(
                "open a missing file",
                r#"{"op":"open","params":{"path":"missing.ron"}}"#,
                OpName::Open,
                true,
            ),
            case(
                "non-ASCII path",
                r#"{"op":"open","params":{"path":"ü.ron"}}"#,
                OpName::Open,
                true,
            ),
            case(
                "inspect, no document",
                r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"]}}"#,
                OpName::Inspect,
                true,
            ),
            case(
                "undo, no document",
                r#"{"op":"undo","params":{"doc_id":"d1","expected_revision":0}}"#,
                OpName::Undo,
                true,
            ),
            case(
                "save, no document",
                r#"{"op":"save","params":{"doc_id":"d1","path":"x.ron"}}"#,
                OpName::Save,
                true,
            ),
            case(
                "request_status",
                r#"{"op":"request_status","params":{"idempotency_key":"k1"}}"#,
                OpName::RequestStatus,
                true,
            ),
            case("malformed JSON", r#"{"op":"#, new, false),
            case("not an object", "[]", new, false),
            case("a string", r#""new""#, new, false),
            case(
                "duplicate key",
                r#"{"op":"new","op":"new","params":{}}"#,
                new,
                false,
            ),
            case(
                "a fraction",
                r#"{"op":"nets","params":{"doc_id":"d1","at":[0.5,0]}}"#,
                OpName::Nets,
                false,
            ),
            case("missing op", r#"{"params":{}}"#, new, true),
            case("missing params", r#"{"op":"new"}"#, new, true),
            case(
                "unknown operation",
                r#"{"op":"frobnicate","params":{}}"#,
                new,
                true,
            ),
            case(
                "unknown envelope field",
                r#"{"op":"new","params":{},"extra":1}"#,
                new,
                true,
            ),
            case(
                "refused key",
                r#"{"op":"capabilities","params":{},"idempotency_key":"k"}"#,
                OpName::Capabilities,
                true,
            ),
            case(
                "params violation",
                r#"{"op":"inspect","params":{"doc_id":"d1","include":["all"]}}"#,
                OpName::Inspect,
                true,
            ),
            case(
                "AK4 page size",
                r#"{"op":"inspect","params":{"doc_id":"d1","include":["items"],"limit":5001}}"#,
                OpName::Inspect,
                true,
            ),
            // AK2 at the limit and one byte over.
            Case {
                name: "exactly AK2 bytes",
                request: padded(MAX_REQUEST_BYTES),
                op: new,
                echoes_id: false,
            },
            Case {
                name: "one byte over AK2",
                request: padded(MAX_REQUEST_BYTES + 1),
                op: new,
                echoes_id: false,
            },
        ]
    }

    /// The response bytes are a valid response of `op`: they decode under its published
    /// schema (AINV11), once any JSON-lines `id` is taken off.
    fn assert_response(op: OpName, line: &str, context: &str) {
        let mut value = parse_strict(line.as_bytes()).expect("a response is strict JSON");
        if let Value::Object(map) = &mut value {
            map.remove("id");
        }
        if let Err(e) = Response::from_value(op, value) {
            panic!("{context}: not a valid {op} response: {e}\n{line}");
        }
    }

    // ---- AD1: the same request through `call` and `serve` ----

    #[test]
    fn a1_7_ad1_call_and_serve_give_byte_identical_responses() {
        let root = TempRoot::new("ad1");
        write_fixture(root.path());
        for case in corpus() {
            let name = case.name;
            let by_arg = call_arg(root.path(), &case.request);
            let by_stdin = call_stdin(root.path(), case.request.as_bytes());
            let by_line = serve(root.path(), format!("{}\n", case.request).as_bytes());
            let by_crlf = serve(root.path(), format!("{}\r\n", case.request).as_bytes());
            let by_last = serve(root.path(), case.request.as_bytes());

            let text = by_arg.text();
            assert_eq!(text.lines().count(), 1, "{name}: one line\n{text}");
            assert!(text.ends_with('\n'), "{name}");
            assert_response(case.op, text.trim_end(), name);
            let ok = field(text, "").get("result").is_some();
            let code = if ok { EXIT_OK } else { EXIT_FAILURE };
            assert_eq!(by_arg.code, code, "{name}");
            assert!(by_arg.stderr.is_empty(), "{name}: call wrote to stderr");

            for (form, other) in [
                ("call -", &by_stdin),
                ("serve", &by_line),
                ("serve, CRLF", &by_crlf),
                ("serve, no final newline", &by_last),
            ] {
                assert_eq!(other.text(), text, "{name}: {form} differs from call");
                assert!(other.stderr.is_empty(), "{name}: {form} wrote to stderr");
            }
            assert_eq!(by_stdin.code, code, "{name}");
            assert_eq!(by_line.code, EXIT_OK, "{name}: serve ends cleanly");

            // The same line with an `id`: when it decodes far enough to have one, the echo is
            // the only difference. When it does not, the line is answered as `call` answers
            // the same bytes (a parse position or an AK2 length counts the id too).
            let ids: &[&str] = if case.request.trim_start().starts_with('{') {
                &["7", r#""req-7""#, "-1"]
            } else {
                &[] // nowhere to put an id
            };
            for id in ids {
                let line = with_id(id, &case.request);
                let echoed = serve(root.path(), format!("{line}\n").as_bytes());
                let expected = if case.echoes_id {
                    format!("{}\n", with_id(id, text.trim_end()))
                } else {
                    call_arg(root.path(), &line).text().to_owned()
                };
                assert_eq!(echoed.text(), expected, "{name}: id {id}");
                assert!(
                    echoed.text().starts_with("{\"id\":") == case.echoes_id,
                    "{name}: id {id}"
                );
            }
        }
    }

    // ---- `serve` is the service and nothing more ----

    /// The session transcript: input lines and the lines `serve` must print.
    #[derive(Default)]
    struct Transcript {
        input: Vec<String>,
        output: String,
    }

    /// Answers `envelope` on the in-process `service` and records the line `serve` must print
    /// for it, with `id` echoed. Returns the response without the id.
    fn step(
        service: &mut AgentService,
        session: omnia_agent::SessionId,
        transcript: &mut Transcript,
        id: Option<&str>,
        op: OpName,
        envelope: &str,
    ) -> String {
        let response = match Request::from_slice(envelope.as_bytes()) {
            Ok(request) => service.handle(session, &request).to_json(),
            Err(error) => jsonl::refusal(error).to_json(),
        };
        assert_response(op, &response, envelope);
        let (line, answer) = match id {
            Some(id) => (with_id(id, envelope), with_id(id, &response)),
            None => (envelope.to_owned(), response.clone()),
        };
        transcript.input.push(line);
        transcript.output.push_str(&answer);
        transcript.output.push('\n');
        response
    }

    /// The ATC-edit-01 loop and the A1.6 file flow as one session: every line `serve` prints
    /// equals what the service answers in-process on one session, plus the echoed `id`. The
    /// session keeps its documents, keys and approvals across lines, also across a bad line.
    #[test]
    fn a1_7_ad1_serve_session_equals_the_service_in_process() {
        let root = TempRoot::new("session");
        let mut service = AgentService::headless_with_roots(Roots::new([root.path()]).unwrap());
        let s = service.open_session();
        let mut t = Transcript::default();
        let svc = &mut service;

        step(
            svc,
            s,
            &mut t,
            Some("1"),
            OpName::New,
            r#"{"op":"new","params":{}}"#,
        );
        let place_both = json!({"op": "apply", "idempotency_key": "k-place", "params":
            {"doc_id": "d1", "expected_revision": 0,
             "operations": [place(0, "R1"), place(12, "R2")]}})
        .to_string();
        let first = step(svc, s, &mut t, None, OpName::Apply, &place_both);
        // AINV7 through the adapter: the replay is byte-identical.
        let replay = step(
            svc,
            s,
            &mut t,
            Some(r#""again""#),
            OpName::Apply,
            &place_both,
        );
        assert_eq!(first, replay);
        let status = r#"{"op":"request_status","params":{"idempotency_key":"k-place"}}"#;
        step(svc, s, &mut t, None, OpName::RequestStatus, status);

        // ATC-edit-01: preview, apply with the preview id, nets, undo.
        let preview = json!({"op": "preview", "params":
            {"doc_id": "d1", "expected_revision": 1, "operations": [wire()]}});
        let preview = step(
            svc,
            s,
            &mut t,
            Some("2"),
            OpName::Preview,
            &preview.to_string(),
        );
        let apply = json!({"op": "apply", "params": {"doc_id": "d1", "expected_revision": 1,
            "operations": [wire()], "preview_id": field(&preview, "/result/preview_id")}});
        let applied = step(svc, s, &mut t, None, OpName::Apply, &apply.to_string());
        assert_eq!(
            field(&applied, "/result/change"),
            field(&preview, "/result/change"),
            "AINV2: apply returns the previewed change"
        );
        // ATC-edit-02: a stale revision is refused.
        let stale = step(svc, s, &mut t, None, OpName::Apply, &apply.to_string());
        assert_eq!(field(&stale, "/error/code"), json!("stale_revision"));
        let nets = r#"{"op":"nets","params":{"doc_id":"d1","wire":"wire:0"}}"#;
        step(svc, s, &mut t, None, OpName::Nets, nets);
        let undo = r#"{"op":"undo","params":{"doc_id":"d1","expected_revision":2}}"#;
        step(svc, s, &mut t, None, OpName::Undo, undo);
        let redo = r#"{"op":"redo","params":{"doc_id":"d1","expected_revision":3}}"#;
        step(svc, s, &mut t, None, OpName::Redo, redo);

        // A1.6 files: a new file (tier 2), then an overwrite challenged at tier 3 and echoed.
        let save = r#"{"op":"save","params":{"doc_id":"d1","path":"session.ron"}}"#;
        step(svc, s, &mut t, None, OpName::Save, save);
        let again = r#"{"op":"save","params":{"doc_id":"d1"}}"#;
        let challenge = step(svc, s, &mut t, Some("3"), OpName::Save, again);
        assert_eq!(field(&challenge, "/error/code"), json!("approval_required"));
        let echo = json!({"op": "save", "params": {"doc_id": "d1", "approval": {
            "approval_id": field(&challenge, "/error/details/approval_id"),
            "effect_digest": field(&challenge, "/error/details/effect_digest")}}});
        let saved = step(svc, s, &mut t, Some("4"), OpName::Save, &echo.to_string());
        assert!(field(&saved, "").get("result").is_some(), "{saved}");

        // A bad line is answered and the session goes on.
        step(svc, s, &mut t, None, OpName::New, r#"{"op":"#);
        let close = r#"{"op":"close","params":{"doc_id":"d1"}}"#;
        step(svc, s, &mut t, None, OpName::Close, close);
        let open = r#"{"op":"open","params":{"path":"session.ron"}}"#;
        step(svc, s, &mut t, None, OpName::Open, open);
        let inspect = r#"{"op":"inspect","params":{"doc_id":"d2","include":["summary","nets"]}}"#;
        let last = step(svc, s, &mut t, Some(r#""last""#), OpName::Inspect, inspect);
        assert_eq!(field(&last, "/result/summary/wires"), json!(1));
        service.close_session(s);

        // The in-process run wrote session.ron; `serve` starts from the same files.
        fs::remove_file(root.path().join("session.ron")).unwrap();
        let mut input = String::new();
        for (i, line) in t.input.iter().enumerate() {
            input.push_str(line);
            // Mixed line ends, and blank lines, which `serve` skips.
            input.push_str(if i % 2 == 0 { "\n" } else { "\r\n" });
            if i % 3 == 0 {
                input.push_str(" \t\r\n\n");
            }
        }
        let out = serve(root.path(), input.as_bytes());
        assert_eq!(out.code, EXIT_OK);
        assert!(out.stderr.is_empty(), "serve wrote to stderr");
        assert_eq!(out.text(), t.output);
    }

    /// AD1 and AK2 at the end of the input: a carriage return with no `\n` after it is part of
    /// the last line, as it is of `call -`'s input; only a consumed `\r\n` drops it.
    #[test]
    fn a1_7_ad1_a_carriage_return_at_the_end_of_input_is_part_of_the_line() {
        let root = TempRoot::new("eof-cr");
        let base = r#"{"op":"new","params":{}}"#;
        let padded = |len: u64| {
            let pad = usize::try_from(len).unwrap() - base.len() - 1;
            format!("{base}{}\r", " ".repeat(pad))
        };
        let at_limit = padded(MAX_REQUEST_BYTES);
        let over = padded(MAX_REQUEST_BYTES + 1);
        for (name, input) in [
            ("malformed", "{\r".to_owned()),
            ("AK2 bytes, the last a CR", at_limit.clone()),
            ("one over AK2, the last a CR", over.clone()),
        ] {
            let called = call_stdin(root.path(), input.as_bytes());
            let served = serve(root.path(), input.as_bytes());
            assert_eq!(served.text(), called.text(), "{name}");
            assert_eq!(
                served.text(),
                call_arg(root.path(), &input).text(),
                "{name}"
            );
            assert!(
                served.stderr.is_empty() && called.stderr.is_empty(),
                "{name}"
            );
        }
        // The malformed line's error column counts the carriage return.
        let malformed = serve(root.path(), b"{\r");
        let message = field(malformed.text(), "/error/message");
        assert_eq!(
            field(malformed.text(), "/error/code"),
            json!("invalid_request")
        );
        assert!(message.as_str().unwrap().ends_with("column 2"), "{message}");
        // At the limit the CR fits; one byte over, AK2 refuses with the full length.
        assert!(
            field(serve(root.path(), at_limit.as_bytes()).text(), "")
                .get("result")
                .is_some()
        );
        let refused = serve(root.path(), over.as_bytes());
        assert_eq!(
            field(refused.text(), "/error/details"),
            json!({"limit": "request_bytes", "value": MAX_REQUEST_BYTES + 1})
        );
        // With a `\n` after it, the CR is the line end's and is dropped.
        let terminated = serve(root.path(), format!("{over}\n").as_bytes());
        let trimmed = call_arg(root.path(), over.strip_suffix('\r').unwrap());
        assert_eq!(terminated.text(), trimmed.text());
        assert!(field(trimmed.text(), "").get("result").is_some());
        assert_eq!(
            serve(root.path(), b"{\r\n").text(),
            call_arg(root.path(), "{").text()
        );
    }

    #[test]
    fn a1_7_serve_refuses_an_oversized_line_and_goes_on() {
        let root = TempRoot::new("oversized");
        let big = format!(
            r#"{{"op":"new","params":{{}},"pad":"{}"}}"#,
            "a".repeat(9 << 20)
        );
        let input = format!(
            "{}\n{big}\r\n{}\n",
            r#"{"op":"new","params":{}}"#,
            r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"]}}"#
        );
        let out = serve(root.path(), input.as_bytes());
        let lines: Vec<&str> = out.text().lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            field(lines[1], "/error/details"),
            json!({"limit": "request_bytes", "value": big.len()}),
            "AK2 reports the line's full length, without its line end"
        );
        assert_eq!(lines[1], call_arg(root.path(), &big).text().trim_end());
        assert_eq!(
            field(lines[2], "/doc_id"),
            json!("d1"),
            "the session went on"
        );
    }

    #[test]
    fn a1_7_serve_refuses_an_id_that_is_not_a_string_or_integer() {
        let root = TempRoot::new("bad-id");
        for id in ["null", "true", "{}", "[1]", "1.5"] {
            let line = with_id(id, r#"{"op":"new","params":{}}"#);
            let out = serve(root.path(), format!("{line}\n").as_bytes());
            let text = out.text().trim_end();
            if id == "1.5" {
                // A fraction is refused by strict parsing, before the id is looked at.
                assert_eq!(field(text, "/error/code"), json!("invalid_request"));
            } else {
                assert_eq!(
                    field(text, "/error/code"),
                    json!("schema_violation"),
                    "{id}"
                );
                assert_eq!(field(text, "/error/details/path"), json!("/id"), "{id}");
            }
            assert!(!text.starts_with(r#"{"id""#), "{id}: nothing to echo");
            assert_eq!(out.code, EXIT_OK);
        }
        // Strict parsing also refuses a duplicate id.
        let out = serve(
            root.path(),
            b"{\"id\":1,\"id\":2,\"op\":\"new\",\"params\":{}}\n",
        );
        assert_eq!(field(out.text(), "/error/code"), json!("invalid_request"));
    }

    // ---- `capabilities` and `schema` ----

    #[test]
    fn a1_7_capabilities_prints_the_result_of_a_capabilities_call() {
        let out = omnia(&args(&["capabilities"]), b"");
        assert_eq!(out.code, EXIT_OK);
        assert!(out.stderr.is_empty());
        assert_eq!(out.text().lines().count(), 1);
        let root = TempRoot::new("capabilities");
        let called = call_arg(root.path(), r#"{"op":"capabilities","params":{}}"#);
        let catalog = out.text().trim_end();
        assert!(
            called
                .text()
                .contains(&format!("\"result\":{catalog},\"warnings\"")),
            "the catalog is byte for byte the call's result"
        );
    }

    fn committed_schema_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../omnia-agent/schema")
    }

    /// AINV11 through the command line: `omnia schema NAME` prints the committed file.
    #[test]
    fn a1_7_ainv11_schema_prints_the_committed_files() {
        let mut names: Vec<String> = fs::read_dir(committed_schema_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
            .collect();
        names.sort();
        let listed = omnia(&args(&["schema"]), b"");
        assert_eq!(listed.code, EXIT_OK);
        assert_eq!(listed.text().lines().collect::<Vec<_>>(), names);
        for name in &names {
            let path = committed_schema_dir().join(format!("{name}.json"));
            let committed = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
            for arg in [name.clone(), format!("{name}.json")] {
                let out = omnia(&args(&["schema", &arg]), b"");
                assert_eq!(out.code, EXIT_OK, "{arg}");
                assert_eq!(out.text(), committed, "{arg}");
                assert!(out.stderr.is_empty());
            }
        }
    }

    // ---- exit codes and streams ----

    #[test]
    fn a1_7_usage_errors_exit_2_with_nothing_on_stdout() {
        let root = TempRoot::new("usage");
        let missing = root.path().join("missing");
        let file = root.path().join("file.txt");
        fs::write(&file, "x").unwrap();
        let (r, m, f) = (
            root.path().to_str().unwrap(),
            missing.to_str().unwrap(),
            file.to_str().unwrap(),
        );
        let new = r#"{"op":"new","params":{}}"#;
        let cases: Vec<Vec<OsString>> = vec![
            args(&[]),
            args(&["frobnicate"]),
            args(&["mcp"]),
            args(&["capabilities", "extra"]),
            args(&["help", "extra"]),
            args(&["schema", "no.such.schema"]),
            args(&["schema", "request", "response"]),
            args(&["call"]),
            args(&["call", "--root", r]),
            args(&["call", new, new]),
            args(&["call", "--root"]),
            args(&["call", "--verbose", new]),
            args(&["call", "--root", m, new]),
            args(&["call", "--root", f, new]),
            args(&["serve", new]),
            args(&["serve", "--root", m]),
        ];
        for a in cases {
            let out = omnia(&a, new.as_bytes());
            assert_eq!(out.code, EXIT_USAGE, "{a:?}");
            assert!(out.stdout.is_empty(), "{a:?}: usage errors stay off stdout");
            let err = String::from_utf8(out.stderr).unwrap();
            assert!(
                err.starts_with("omnia: ") && err.contains("usage: omnia"),
                "{a:?}: {err}"
            );
        }
        let help = omnia(&args(&["help"]), b"");
        assert_eq!(help.code, EXIT_OK);
        assert_eq!(help.text(), USAGE);
        assert!(help.stderr.is_empty());
    }

    #[test]
    fn a1_7_call_exit_codes_follow_the_outcome() {
        let root = TempRoot::new("exit");
        let ok = call_arg(root.path(), r#"{"op":"new","params":{}}"#);
        assert_eq!(ok.code, EXIT_OK);
        let inspect = r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"]}}"#;
        let tool_error = call_arg(root.path(), inspect);
        assert_eq!(tool_error.code, EXIT_FAILURE);
        assert_eq!(
            field(tool_error.text(), "/error/code"),
            json!("document_not_found")
        );
        let malformed = call_arg(root.path(), "{");
        assert_eq!(
            malformed.code, EXIT_FAILURE,
            "an undecodable request is a tool error"
        );
        let empty = call_stdin(root.path(), b"");
        assert_eq!(empty.code, EXIT_FAILURE);
        assert_eq!(field(empty.text(), "/error/code"), json!("invalid_request"));
        // A `request_status` that reports `not_run` is itself a success.
        let status = r#"{"op":"request_status","params":{"idempotency_key":"k"}}"#;
        assert_eq!(call_arg(root.path(), status).code, EXIT_OK);
        // `--root=DIR` and `--` work like `--root DIR`.
        let mut a = args(&["call"]);
        a.push(OsString::from(format!("--root={}", root.path().display())));
        a.push(OsString::from("--"));
        a.push(OsString::from(r#"{"op":"new","params":{}}"#));
        assert_eq!(omnia(&a, b"").text(), ok.text());
        // A request that starts with `-` reads as an option unless `--` comes first.
        let mut dashed = rooted("call", root.path());
        dashed.push(OsString::from("-1"));
        let refused = omnia(&dashed, b"");
        assert_eq!(refused.code, EXIT_USAGE);
        assert!(refused.stdout.is_empty());
        dashed.insert(3, OsString::from("--"));
        let answered = omnia(&dashed, b"");
        assert_eq!(answered.code, EXIT_FAILURE, "`-1` is not an envelope");
        assert_eq!(answered.text(), serve(root.path(), b"-1\n").text());
        assert_eq!(
            field(answered.text(), "/error/code"),
            json!("invalid_request")
        );
        assert!(USAGE.contains("[--] REQUEST"));
        // No root: every path is outside the roots (section 12.1).
        let open = r#"{"op":"open","params":{"path":"x.ron"}}"#;
        let unrooted = omnia(&args(&["call", open]), b"");
        assert_eq!(unrooted.code, EXIT_FAILURE);
        assert_eq!(
            field(unrooted.text(), "/error/code"),
            json!("path_outside_root")
        );
    }

    /// An output stream that refuses every write.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a1_7_a_closed_stdout_exits_1_with_the_reason_on_stderr() {
        let root = TempRoot::new("closed");
        for command in ["call", "serve"] {
            let mut a = rooted(command, root.path());
            if command == "call" {
                a.push(OsString::from("-"));
            }
            let mut input: &[u8] = b"{\"op\":\"new\",\"params\":{}}\n";
            let mut stderr = Vec::new();
            let code = run(&a, &mut input, &mut Closed, &mut stderr);
            assert_eq!(code, EXIT_FAILURE, "{command}");
            let err = String::from_utf8(stderr).unwrap();
            assert!(err.starts_with("omnia: "), "{command}: {err}");
        }
    }
}
