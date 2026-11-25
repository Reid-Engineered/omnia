# Omnia Agent Evidence Register (`omnia.agent/1`)

This register maps every invariant (`AINV1`–`AINV13`), scenario (`ATC-*`), milestone prerequisite,
and gate of [mcp-plan.md](../../mcp-plan.md) to its verification evidence in the repository.
Every file path and test function name below is checked by `crates/omnia-cli/tests/contract.rs`.

## Invariants (`AINV1`–`AINV13`)

| ID | Rule | Evidence tier | Verification |
| --- | --- | --- | --- |
| `AINV1` | Agent edits reach a document only through `Doc::apply`, `undo` and `redo` | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv1_omnia_agent_calls_no_doc_mutator_but_apply_undo_and_redo`) |
| `AINV2` | Preview `Change`, diff and after-state equal `apply` at the same revision | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv2_preview_equals_commit`) |
| `AINV3` | Read, validate and preview leave the document and history unchanged | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv3_reads_leave_the_document_and_its_history_unchanged`) |
| `AINV4` | A refused request leaves the document unchanged | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv4_a_refused_request_changes_nothing`) |
| `AINV5` | Deterministic, byte-identical responses across insertion order and JSON key order | `implemented and tested` | `crates/omnia-agent/src/read.rs` (`ainv5_reverse_insertion_gives_identical_bytes_up_to_ids`); `crates/omnia-agent/src/service.rs` (`ainv5_the_same_state_and_request_give_the_same_bytes`); `crates/omnia-agent/src/schema/tests.rs` (`ainv5_error_paths_do_not_depend_on_key_order`) |
| `AINV6` | No mutation without an `expected_revision` equal to the current revision | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv6_no_mutation_without_the_current_revision`) |
| `AINV7` | Replaying an idempotency key returns identical bytes and applies at most once | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`ainv7_replaying_a_key_returns_identical_bytes_and_applies_at_most_once`); `crates/omnia-cli/src/mcp.rs` (`a1_8_save_replay_does_not_roll_back_docs_path`) |
| `AINV8` | `omnia-agent` and `omnia-cli` do not depend on GPUI (`INV12`) | `implemented and tested` | `xtask/src/main.rs` (`inv12_document_and_symbols_are_gpui_independent`) |
| `AINV9` | No tier-3 effect occurs without a matching approval echo | `implemented and tested` | `crates/omnia-agent/src/approval.rs` (`ainv9_tier3_requires_matching_echo_and_consumes_grant`); `crates/omnia-agent/src/service.rs` (`ainv9_close_dirty_document_requires_discard_flag_and_matching_approval`, `atc_save_01_and_ainv9_save_new_then_overwrite_with_echoed_digest_and_idempotency`) |
| `AINV10` | Exhaustive `CommandError` mapping gives every variant a distinct error code | `implemented and tested` | `crates/omnia-agent/src/error.rs` (`ainv10_every_command_error_variant_has_its_own_code`, `ainv10_command_error_details_keep_their_ids`) |
| `AINV11` | Catalog equals dispatch table; committed schemas equal generated; contract equals catalog | `implemented and tested` | `crates/omnia-agent/src/catalog.rs` (`ainv11_catalog_rows_follow_the_dispatch_table`); `crates/omnia-agent/src/schema/tests.rs` (`ainv11_committed_schemas_equal_generated`, `ainv11_generation_is_deterministic_and_covers_the_catalog`, `ainv11_decoder_dispatches_exactly_the_catalog`); `crates/omnia-cli/src/main.rs` (`a1_7_ainv11_schema_prints_the_committed_files`); `crates/omnia-cli/src/mcp.rs` (`a1_8_ainv11_tools_list_matches_catalog_and_schemas`); `crates/omnia-cli/tests/contract.rs` (`a1_9_contract_matches_catalog_and_schemas`) |
| `AINV12` | No file outside configured `--root` directories is read or written | `implemented and tested` | `crates/omnia-agent/src/files.rs` (`ainv12_escaping_paths_are_refused_in_windows_and_posix_forms`) |
| `AINV13` | In app mode the revision does not change while a user gesture is active | `planned` | Planned in A4.2 (`crates/omnia-app/src/agent_host.rs`) |

## Scenarios (`ATC-*`)

| ID | Scenario | Evidence tier | Verification |
| --- | --- | --- | --- |
| `ATC-read-01` | Open generated `100` sheet; `inspect` summary matches counts and revision | `implemented and tested` | `crates/omnia-agent/src/read.rs` (`atc_read_01_summary_of_an_opened_100_sheet`) |
| `ATC-net-01` | Two interior-crossing wires form two distinct nets (`INV2`) | `implemented and tested` | `crates/omnia-agent/src/read.rs` (`atc_net_01_wires_crossing_at_interiors_are_two_nets`) |
| `ATC-edit-01` | Place two resistors, preview wire, apply with `preview_id`, verify net, undo | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_01_preview_apply_with_the_preview_id_then_undo`) |
| `ATC-edit-02` | Apply with stale `expected_revision` returns `stale_revision` without mutating | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_02_an_old_expected_revision_is_stale`) |
| `ATC-edit-03` | Replaying same `idempotency_key` returns byte-identical response and applies once | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_03_the_same_key_twice_is_byte_identical_in_both_forms`) |
| `ATC-edit-04` | Batch whose third operation names a missing item rolls back atomically | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_04_a_missing_item_in_the_third_operation_refuses_the_batch`) |
| `ATC-edit-05` | Apply with `preview_id` from different operations returns `preview_mismatch` | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_05_a_preview_id_from_other_operations_is_a_mismatch`) |
| `ATC-edit-06` | Person's edit between agent apply and undo makes agent undo `stale_revision` | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_edit_06_a_persons_edit_between_apply_and_undo_makes_the_undo_stale`) |
| `ATC-save-01` | Overwrite requires `approval_required` echo; failed staged write keeps old file | `implemented and tested` | `crates/omnia-agent/src/service.rs` (`atc_save_01_and_ainv9_save_new_then_overwrite_with_echoed_digest_and_idempotency`) |
| `ATC-path-01` | `..`, outside absolute path, and escaping symlink all return `path_outside_root` | `implemented and tested` | `crates/omnia-agent/src/files.rs` (`atc_path_01_refuses_traversal_outside_absolute_and_symlink_escapes`); `crates/omnia-agent/src/service.rs` (`atc_path_01_service_refuses_outside_paths_without_reading`) |
| `ATC-mcp-01` | Handshake, `tools/list`, and `ATC-edit-01` loop over MCP stdio | `implemented and tested` | `crates/omnia-cli/src/mcp.rs` (`a1_8_exact_transcript_fixtures`); `crates/omnia-cli/tests/transcripts/atc-mcp-01.in.jsonl`, `crates/omnia-cli/tests/transcripts/atc-mcp-01.out.jsonl` |
| `ATC-mcp-02` | Malformed JSON-RPC error vs domain failure with `isError: true` | `implemented and tested` | `crates/omnia-cli/src/mcp.rs` (`a1_8_exact_transcript_fixtures`); `crates/omnia-cli/tests/transcripts/atc-mcp-02.in.jsonl`, `crates/omnia-cli/tests/transcripts/atc-mcp-02.out.jsonl` |

## Prerequisites, transports and protocol eras

| Area | Evidence tier | Verification |
| --- | --- | --- |
| `Doc::fork()` (`A1.1`, `AD3`) | `implemented and tested` | `crates/omnia-doc/src/doc.rs` (`a1_1_fork_keeps_reserved_slots_and_revision_with_empty_history`, `a1_1_fork_keeps_hash_map_order_and_exact_net_ids`, `a1_1_applying_to_a_fork_leaves_the_original_untouched`, `a1_1_fork_equals_original_and_commits_identically`) |
| `SymbolLibrary::place_command` (`A1.2`, `AD11`) | `implemented and tested` | `crates/omnia-symbols/src/lib.rs` (`a1_2_place_command_world_pins_for_every_symbol_and_orientation`) |
| CLI `capabilities`, `schema`, `call`, `serve` (`A1.7`, `AD1`) | `implemented and tested` | `crates/omnia-cli/src/main.rs` (`a1_7_ad1_call_and_serve_give_byte_identical_responses`, `a1_7_ad1_serve_session_equals_the_service_in_process`, `a1_7_capabilities_prints_the_result_of_a_capabilities_call`, `a1_7_usage_errors_exit_2_with_nothing_on_stdout`, `a1_7_call_exit_codes_follow_the_outcome`) |
| Dual-era MCP (`2026-07-28`, `2025-11-25`, `2025-06-18`), no tasks (`AQ15`), resources, prompts, `AD14` (`A1.8`) | `implemented and tested` | `crates/omnia-cli/src/mcp.rs` (`a1_8_handshake_versions_and_aq15_no_tasks`, `a1_8_stateless_discover_version_error_and_aq15`, `a1_8_one_era_per_process_and_no_guessing`, `a1_8_ad1_tools_call_structured_content_strict_arguments_and_errors`, `a1_8_resources_all_uris_and_handshake_subscriptions`, `a1_8_logging_filtering_and_ad14_no_document_text_leak`, `a1_8_ak2_oversized_mcp_line_refused_and_session_continues`); `crates/omnia-cli/tests/transcripts/stateless-era.in.jsonl`, `crates/omnia-cli/tests/transcripts/stateless-era.out.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-inspect-sheet.in.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-inspect-sheet.out.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-place-and-wire.in.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-place-and-wire.out.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-safe-edit-loop.in.jsonl`, `crates/omnia-cli/tests/transcripts/prompt-safe-edit-loop.out.jsonl` |
| Contract-to-catalog parity (`A1.9`) | `implemented and tested` | `crates/omnia-cli/tests/contract.rs` (`a1_9_contract_matches_catalog_and_schemas`, `a1_9_contract_check_detects_missing_and_extra_entries`, `a1_9_transcript_fixtures_are_complete_and_valid`, `a1_9_evidence_register_paths_and_tests_exist`) |

## Performance measurements and milestone gates

| Budget / gate | Status | Record |
| --- | --- | --- |
| `A1.1` `Doc::fork` on `10k` (non-reference Windows diagnostic) | Recorded (certifies nothing; reference machine required for budgets) | `docs/measurements/A1.1.md` |
| `AB1` (`inspect` summary on `10k`, p99 < 5 ms) & `AB2` (`preview` 100-part `Move` on `10k`, p99 < 25 ms) | `planned` (Milestone A1 gate `A1.10`, human checkpoint `HA2`) | `docs/measurements/A1.10.md` (not yet created) |
| `AB3` (app mode `1k` pan under agent load) | `planned` (Milestone A4 gate `A4.5`, human checkpoint `HA5`) | `docs/measurements/A4.5.md` (not yet created) |
