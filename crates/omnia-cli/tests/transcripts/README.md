# MCP transcripts (A1.8)

Exact request/response fixtures for `omnia mcp`, run by the tests in
`crates/omnia-cli/src/mcp.rs`. Each `<name>.in.jsonl` is fed to the server on stdin and
its stdout must equal `<name>.expected.jsonl` byte for byte.

| Fixture | Scenario |
| --- | --- |
| `atc-mcp-01` | ATC-mcp-01: handshake (2025-06-18), `tools/list`, then the ATC-edit-01 loop over MCP (new, apply, preview, apply with the preview id, nets, subscribe, undo, inspect, resources) |
| `atc-mcp-02` | ATC-mcp-02: an unknown tool and a malformed JSON-RPC message are protocol errors; a tool call that fails in the domain is a result with `isError` and the structured error |
| `atc-mcp-03-stateless` | The stateless era (2026-07-28): `server/discover`, per-request `_meta`, caching hints, `subscriptions/listen`-tagged update notifications, era and version refusals |
| `prompts` | Every prompt of mcp-plan.md section 10.3: `prompts/list` and a `prompts/get` fixture for each |

Regenerate an expected file only after reading the diff line by line:

```sh
cargo build -p omnia-cli
omnia mcp < atc-mcp-01.in.jsonl > atc-mcp-01.expected.jsonl
```
