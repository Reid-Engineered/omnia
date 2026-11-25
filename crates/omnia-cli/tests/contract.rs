//! A1.9 contract drift checks (mcp-plan.md section 13, AINV11).
//!
//! Fails if any documented tool, operation, edit operation, error code, count, unit, limit,
//! schema count, transcript fixture reference, or evidence-register entry disagrees with the
//! live `Catalog::current()` (`omnia capabilities`) or the repository.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use omnia_agent::{
    Catalog, PROTOCOL_REVISIONS, SCHEMA,
    schema::{ORIENT_NAMES, TEXT_FIELD_NAMES, schema_files},
};

const VALID_TIERS: [&str; 3] = [
    "implemented and tested",
    "implemented (read in code)",
    "planned",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_text(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Splits one markdown table row into trimmed cells, preserving escaped `\|`.
fn split_markdown_row(row: &str) -> Vec<String> {
    let inner = row.trim().trim_matches('|');
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            chars.next();
            current.push('|');
        } else if ch == '|' {
            cells.push(current.trim().to_owned());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    cells.push(current.trim().to_owned());
    cells
}

/// Extracts data rows (excluding header and separator) from the first markdown table under
/// `heading` (matched as an exact trimmed line), stopping at the next heading.
fn table_under(doc: &str, heading: &str) -> Result<Vec<Vec<String>>, String> {
    let mut in_section = false;
    let mut table_lines = Vec::new();
    for line in doc.lines() {
        let trimmed = line.trim();
        if !in_section {
            if trimmed == heading {
                in_section = true;
            }
            continue;
        }
        if trimmed.starts_with('#') {
            break;
        }
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            table_lines.push(trimmed);
        } else if !table_lines.is_empty() {
            break;
        }
    }
    if table_lines.len() < 3 {
        return Err(format!(
            "expected markdown table with header, separator and rows under `{heading}`"
        ));
    }
    Ok(table_lines[2..]
        .iter()
        .map(|row| split_markdown_row(row))
        .collect())
}

fn strip_code(cell: &str) -> Result<&str, String> {
    cell.trim()
        .strip_prefix('`')
        .and_then(|s| s.strip_suffix('`'))
        .ok_or_else(|| format!("expected backtick-wrapped value, got `{cell}`"))
}

fn extract_backtick_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('`') {
            out.push(&after[..end]);
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn parse_tiers(cell: &str) -> Result<Vec<u8>, String> {
    strip_code(cell)?
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<u8>()
                .map_err(|e| format!("invalid tier in `{cell}`: {e}"))
        })
        .collect()
}

fn parse_bool(cell: &str) -> Result<bool, String> {
    match strip_code(cell)? {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected `true` or `false`, got `{other}`")),
    }
}

fn check_contract_against_catalog(
    contract: &str,
    catalog: &Catalog,
    schema_count: usize,
    transcript_pair_count: usize,
) -> Result<(), String> {
    // 1. Operations and MCP tools table: symmetric equality + field parity.
    let op_rows = table_under(contract, "### Operations and MCP tools")?;
    let mut doc_ops = BTreeSet::new();
    let mut doc_tools = BTreeSet::new();
    let cat_by_op: BTreeMap<_, _> = catalog
        .operations
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();
    for row in &op_rows {
        if row.len() != 9 {
            return Err(format!("unexpected operations row width: {row:?}"));
        }
        let op = strip_code(&row[0])?;
        let tool = strip_code(&row[1])?;
        let tiers = parse_tiers(&row[2])?;
        let read_only = parse_bool(&row[3])?;
        let destructive = parse_bool(&row[4])?;
        let idempotent = parse_bool(&row[5])?;
        let keyed = parse_bool(&row[6])?;
        let schemas = extract_backtick_tokens(&row[7]);
        let evidence = strip_code(&row[8])?;
        if !VALID_TIERS.contains(&evidence) {
            return Err(format!("invalid evidence tier `{evidence}` for `{op}`"));
        }
        if !doc_ops.insert(op.to_owned()) {
            return Err(format!("duplicate documented operation `{op}`"));
        }
        if !doc_tools.insert(tool.to_owned()) {
            return Err(format!("duplicate documented MCP tool `{tool}`"));
        }

        let entry = cat_by_op
            .get(op)
            .ok_or_else(|| format!("documented operation `{op}` is missing from the catalog"))?;
        if entry.tool != tool {
            return Err(format!(
                "MCP tool mismatch for `{op}`: documented `{tool}`, catalog `{}`",
                entry.tool
            ));
        }
        if entry.tiers != tiers {
            return Err(format!("tiers mismatch for `{op}`"));
        }
        if entry.read_only != read_only {
            return Err(format!("read_only mismatch for `{op}`"));
        }
        if entry.destructive != destructive {
            return Err(format!("destructive mismatch for `{op}`"));
        }
        if entry.idempotent != idempotent {
            return Err(format!("idempotent mismatch for `{op}`"));
        }
        if entry.idempotency_key != keyed {
            return Err(format!("idempotency_key mismatch for `{op}`"));
        }
        let expected_schemas = vec![
            entry.params_schema.as_str(),
            entry.arguments_schema.as_str(),
            entry.response_schema.as_str(),
        ];
        if schemas != expected_schemas {
            return Err(format!("schema names mismatch for `{op}`"));
        }
    }
    let cat_ops: BTreeSet<String> = catalog
        .operations
        .iter()
        .map(|e| e.name.as_str().to_owned())
        .collect();
    let cat_tools: BTreeSet<String> = catalog.operations.iter().map(|e| e.tool.clone()).collect();
    if doc_ops != cat_ops {
        return Err(format!(
            "documented operations and catalog operations disagree: doc={doc_ops:?}, cat={cat_ops:?}"
        ));
    }
    if doc_tools != cat_tools {
        return Err(format!(
            "documented MCP tools and catalog MCP tools disagree: doc={doc_tools:?}, cat={cat_tools:?}"
        ));
    }

    // 2. Edit operations table: symmetric equality + Command parity.
    let edit_rows = table_under(contract, "### Edit operations")?;
    let mut doc_edits = BTreeSet::new();
    let cat_edits_by_name: BTreeMap<_, _> = catalog
        .edit_operations
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();
    for row in &edit_rows {
        if row.len() != 4 {
            return Err(format!("unexpected edit operations row width: {row:?}"));
        }
        let name = strip_code(&row[0])?;
        let command = strip_code(&row[1])?;
        let evidence = strip_code(&row[3])?;
        if !VALID_TIERS.contains(&evidence) {
            return Err(format!(
                "invalid evidence tier `{evidence}` for edit `{name}`"
            ));
        }
        if !doc_edits.insert(name.to_owned()) {
            return Err(format!("duplicate documented edit operation `{name}`"));
        }
        let entry = cat_edits_by_name.get(name).ok_or_else(|| {
            format!("documented edit operation `{name}` is missing from the catalog")
        })?;
        if entry.command != command {
            return Err(format!("Command mismatch for edit operation `{name}`"));
        }
    }
    let cat_edits: BTreeSet<String> = catalog
        .edit_operations
        .iter()
        .map(|e| e.name.as_str().to_owned())
        .collect();
    if doc_edits != cat_edits {
        return Err(format!(
            "documented edit operations and catalog edit operations disagree: doc={doc_edits:?}, cat={cat_edits:?}"
        ));
    }

    // 3. Error codes table: symmetric equality + retryable parity.
    let err_rows = table_under(contract, "### Error codes")?;
    let mut doc_errs = BTreeSet::new();
    let cat_errs_by_code: BTreeMap<_, _> = catalog
        .error_codes
        .iter()
        .map(|e| (e.code.as_str(), e))
        .collect();
    for row in &err_rows {
        if row.len() != 4 {
            return Err(format!("unexpected error codes row width: {row:?}"));
        }
        let code = strip_code(&row[0])?;
        let retryable = parse_bool(&row[1])?;
        let evidence = strip_code(&row[3])?;
        if !VALID_TIERS.contains(&evidence) {
            return Err(format!(
                "invalid evidence tier `{evidence}` for error `{code}`"
            ));
        }
        if !doc_errs.insert(code.to_owned()) {
            return Err(format!("duplicate documented error code `{code}`"));
        }
        let entry = cat_errs_by_code
            .get(code)
            .ok_or_else(|| format!("documented error code `{code}` is missing from catalog"))?;
        if entry.retryable != retryable {
            return Err(format!("retryable mismatch for error code `{code}`"));
        }
    }
    let cat_errs: BTreeSet<String> = catalog
        .error_codes
        .iter()
        .map(|e| e.code.as_str().to_owned())
        .collect();
    if doc_errs != cat_errs {
        return Err(format!(
            "documented error codes and catalog error codes disagree: doc={doc_errs:?}, cat={cat_errs:?}"
        ));
    }

    // 4. Planned capabilities table: every row must be marked `planned` and absent from catalog.
    let planned_rows = table_under(contract, "### Planned capabilities")?;
    for row in &planned_rows {
        if row.len() != 3 {
            return Err(format!("unexpected planned row width: {row:?}"));
        }
        if strip_code(&row[2])? != "planned" {
            return Err(format!(
                "planned table entry must carry `planned` evidence tier: {row:?}"
            ));
        }
        for token in extract_backtick_tokens(&row[0]) {
            if cat_tools.contains(token) {
                return Err(format!(
                    "planned tool `{token}` is now active in the catalog; move it to active tables"
                ));
            }
        }
    }

    // 5. Catalog counts table: verify every reported count matches live catalog / files.
    let count_rows = table_under(contract, "## Catalog counts")?;
    let mut counts = BTreeMap::new();
    for r in &count_rows {
        if r.len() < 2 {
            return Err(format!("unexpected count row: {r:?}"));
        }
        counts.insert(r[0].clone(), strip_code(&r[1])?.to_owned());
    }
    let retryable_count = catalog.error_codes.iter().filter(|e| e.retryable).count();
    let expected_counts = [
        ("Schema version", SCHEMA.to_owned()),
        (
            "Pinned MCP protocol revisions",
            PROTOCOL_REVISIONS.len().to_string(),
        ),
        (
            "Available operations / MCP tools",
            catalog.operations.len().to_string(),
        ),
        ("Edit operations", catalog.edit_operations.len().to_string()),
        (
            "Structured error codes",
            catalog.error_codes.len().to_string(),
        ),
        ("Retryable error codes", retryable_count.to_string()),
        ("Orientations", ORIENT_NAMES.len().to_string()),
        ("Text fields", TEXT_FIELD_NAMES.len().to_string()),
        ("Committed JSON Schema files", schema_count.to_string()),
        (
            "Exact CI transcript fixture pairs",
            transcript_pair_count.to_string(),
        ),
    ];
    for (label, expected) in expected_counts {
        let actual = counts
            .get(label)
            .ok_or_else(|| format!("missing count row `{label}`"))?;
        if actual != &expected {
            return Err(format!(
                "count mismatch for `{label}`: documented `{actual}`, catalog `{expected}`"
            ));
        }
    }

    // 6. Units and limits table + protocol revisions, orientations & text fields.
    let unit_rows = table_under(contract, "## Units and limits")?;
    let mut units = BTreeMap::new();
    for r in &unit_rows {
        if r.len() < 3 {
            return Err(format!("unexpected unit/limit row: {r:?}"));
        }
        let key = strip_code(&r[1])?.to_owned();
        let val = strip_code(&r[2])?
            .parse::<u64>()
            .map_err(|e| format!("non-integer constant for `{key}`: {e}"))?;
        units.insert(key, val);
    }
    let expected_units = [
        ("units.grid_nm", u64::from(catalog.units.grid_nm)),
        (
            "units.pin_lattice_nm",
            u64::from(catalog.units.pin_lattice_nm),
        ),
        (
            "units.sheet_limit_nm",
            u64::from(catalog.units.sheet_limit_nm),
        ),
        ("limits.request_bytes", catalog.limits.request_bytes),
        ("limits.file_bytes", catalog.limits.file_bytes),
        (
            "limits.operations_per_request",
            catalog.limits.operations_per_request,
        ),
        (
            "limits.default_page_size",
            u64::from(catalog.limits.default_page_size),
        ),
        (
            "limits.max_page_size",
            u64::from(catalog.limits.max_page_size),
        ),
        (
            "limits.idempotency_cache_entries",
            u64::from(catalog.limits.idempotency_cache_entries),
        ),
        (
            "limits.open_documents",
            u64::from(catalog.limits.open_documents),
        ),
    ];
    for (key, expected) in expected_units {
        let actual = units
            .get(key)
            .copied()
            .ok_or_else(|| format!("missing unit/limit row `{key}`"))?;
        if actual != expected {
            return Err(format!(
                "unit/limit mismatch for `{key}`: documented {actual}, catalog {expected}"
            ));
        }
    }

    for rev in &catalog.protocol_revisions {
        if !contract.contains(rev) {
            return Err(format!("contract missing protocol revision `{rev}`"));
        }
    }
    for orient in &catalog.orientations {
        if !contract.contains(&format!("`{orient}`")) {
            return Err(format!("contract missing orientation `{orient}`"));
        }
    }
    for field in &catalog.text_fields {
        if !contract.contains(&format!("`{field}`")) {
            return Err(format!("contract missing text field `{field}`"));
        }
    }
    Ok(())
}

fn transcript_fixture_pairs() -> BTreeSet<String> {
    let dir = repo_root().join("crates/omnia-cli/tests/transcripts");
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in fs::read_dir(&dir).expect("read transcripts dir") {
        let name = entry
            .expect("transcript dir entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if let Some(stem) = name.strip_suffix(".in.jsonl") {
            inputs.insert(stem.to_owned());
        } else if let Some(stem) = name.strip_suffix(".out.jsonl") {
            outputs.insert(stem.to_owned());
        } else {
            panic!("unexpected file in transcripts dir: {name}");
        }
    }
    assert_eq!(inputs, outputs, "mismatched .in.jsonl / .out.jsonl pairs");
    assert!(!inputs.is_empty(), "expected transcript fixtures");
    inputs
}

#[test]
fn a1_9_contract_matches_catalog_and_schemas() {
    // Verify `omnia capabilities` CLI matches `Catalog::current()` byte-for-byte.
    let output = Command::new(env!("CARGO_BIN_EXE_omnia"))
        .arg("capabilities")
        .output()
        .expect("run `omnia capabilities`");
    assert!(
        output.status.success(),
        "`omnia capabilities` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_json = String::from_utf8(output.stdout).expect("utf-8 capabilities output");
    let catalog = Catalog::current();
    let expected_json = serde_json::to_string(&catalog).expect("serialize catalog");
    assert_eq!(
        cli_json.trim_end_matches(&['\r', '\n'][..]),
        expected_json,
        "`omnia capabilities` output must match `Catalog::current()`"
    );

    let contract = read_text("docs/agent/omnia-agent.md");
    let changes = read_text("docs/agent/CHANGES.md");
    assert!(
        changes.contains(SCHEMA),
        "docs/agent/CHANGES.md must name current schema `{SCHEMA}`"
    );

    check_contract_against_catalog(
        &contract,
        &catalog,
        schema_files().len(),
        transcript_fixture_pairs().len(),
    )
    .expect("contract must agree with live catalog");
}

#[test]
fn a1_9_contract_check_detects_missing_and_extra_entries() {
    let catalog = Catalog::current();
    let schema_count = schema_files().len();
    let pair_count = transcript_fixture_pairs().len();
    let contract = read_text("docs/agent/omnia-agent.md");

    // Helper: drop one line matching `needle`.
    let drop_line = |needle: &str| -> String {
        contract
            .lines()
            .filter(|line| !line.contains(needle))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // 1. Missing documented operation/tool (in catalog, missing from contract).
    assert!(
        check_contract_against_catalog(
            &drop_line("| `redo` |"),
            &catalog,
            schema_count,
            pair_count
        )
        .is_err(),
        "dropping a catalog operation/tool must fail"
    );
    // 2. Extra documented operation/tool (in contract, missing from catalog).
    let extra_op = contract.replace(
        "| `request_status` |",
        "| `bogus_op` | `omnia_bogus_op` | `0` | `true` | `false` | `true` | `false` | `bogus_op.params`, `bogus_op.arguments`, `bogus_op.response` | `implemented and tested` |\n| `request_status` |",
    );
    assert!(
        check_contract_against_catalog(&extra_op, &catalog, schema_count, pair_count).is_err(),
        "adding a non-catalog operation/tool must fail"
    );
    // 3. Mismatched MCP tool name.
    let bad_tool = contract.replace("| `omnia_nets` |", "| `omnia_wrong_nets` |");
    assert!(
        check_contract_against_catalog(&bad_tool, &catalog, schema_count, pair_count).is_err(),
        "mismatching a tool name must fail"
    );
    // 4. Missing documented edit operation.
    assert!(
        check_contract_against_catalog(
            &drop_line("| `set_text` |"),
            &catalog,
            schema_count,
            pair_count
        )
        .is_err(),
        "dropping a catalog edit operation must fail"
    );
    // 5. Extra documented edit operation.
    let extra_edit = contract.replace(
        "| `set_text` |",
        "| `bogus_edit` | `BogusEdit` | `items` | `implemented and tested` |\n| `set_text` |",
    );
    assert!(
        check_contract_against_catalog(&extra_edit, &catalog, schema_count, pair_count).is_err(),
        "adding a non-catalog edit operation must fail"
    );
    // 6. Missing documented error code.
    assert!(
        check_contract_against_catalog(
            &drop_line("| `internal` |"),
            &catalog,
            schema_count,
            pair_count
        )
        .is_err(),
        "dropping a catalog error code must fail"
    );
    // 7. Extra documented error code.
    let extra_err = contract.replace(
        "| `internal` |",
        "| `bogus_error` | `false` | `{}` | `implemented and tested` |\n| `internal` |",
    );
    assert!(
        check_contract_against_catalog(&extra_err, &catalog, schema_count, pair_count).is_err(),
        "adding a non-catalog error code must fail"
    );
    // 8. Wrong retryable flag on an error code.
    let wrong_retry = contract.replace(
        "| `stale_revision` | `true` |",
        "| `stale_revision` | `false` |",
    );
    assert!(
        check_contract_against_catalog(&wrong_retry, &catalog, schema_count, pair_count).is_err(),
        "mismatching retryable flag must fail"
    );
    // 9. Stale documented count.
    let wrong_count = contract.replace(
        "| Structured error codes | `33` |",
        "| Structured error codes | `32` |",
    );
    assert!(
        check_contract_against_catalog(&wrong_count, &catalog, schema_count, pair_count).is_err(),
        "stale error code count must fail"
    );
}

#[test]
fn a1_9_transcript_fixtures_are_complete_and_valid() {
    let root = repo_root();
    let contract = read_text("docs/agent/omnia-agent.md");
    let evidence = read_text("docs/agent/evidence.md");
    let pairs = transcript_fixture_pairs();

    // Every transcript fixture pair on disk must be pointed to by the contract.
    for stem in &pairs {
        let in_rel = format!("crates/omnia-cli/tests/transcripts/{stem}.in.jsonl");
        let out_rel = format!("crates/omnia-cli/tests/transcripts/{stem}.out.jsonl");
        assert!(
            contract.contains(&in_rel),
            "docs/agent/omnia-agent.md must point to transcript fixture `{in_rel}`"
        );
        assert!(
            contract.contains(&out_rel),
            "docs/agent/omnia-agent.md must point to transcript fixture `{out_rel}`"
        );
    }

    // Every transcript path mentioned in either doc must exist on disk.
    for (name, text) in [
        ("docs/agent/omnia-agent.md", &contract),
        ("docs/agent/evidence.md", &evidence),
    ] {
        for token in extract_backtick_tokens(text) {
            if token.starts_with("crates/omnia-cli/tests/transcripts/") && !token.ends_with('/') {
                assert!(
                    root.join(token).is_file(),
                    "{name} references missing transcript fixture `{token}`"
                );
            }
        }
    }
}

#[test]
fn a1_9_evidence_register_paths_and_tests_exist() {
    let root = repo_root();
    let evidence = read_text("docs/agent/evidence.md");

    for heading in [
        "## Invariants (`AINV1`–`AINV13`)",
        "## Scenarios (`ATC-*`)",
        "## Prerequisites, transports and protocol eras",
    ] {
        let rows = table_under(&evidence, heading).expect("evidence table");
        for row in &rows {
            let tier_col = if row.len() == 4 { 2 } else { 1 };
            let tier = strip_code(&row[tier_col]).expect("tier");
            assert!(
                VALID_TIERS.contains(&tier),
                "invalid evidence tier `{tier}` in `{heading}` row: {row:?}"
            );
            if tier == "planned" {
                continue;
            }
            let verification = row.last().expect("verification column");
            // Each semicolon-separated segment has `<file>` optionally followed by (`test1`, `test2`).
            for segment in verification.split(';') {
                let segment = segment.trim();
                let tokens = extract_backtick_tokens(segment);
                assert!(
                    !tokens.is_empty(),
                    "missing path/test citation in `{heading}`: {segment}"
                );
                let mut current_file: Option<PathBuf> = None;
                let mut current_src: Option<String> = None;
                for token in tokens {
                    let is_path = token.contains('/');
                    if is_path {
                        let path = root.join(token);
                        assert!(
                            path.is_file(),
                            "evidence register cites missing file `{token}`"
                        );
                        if Path::new(token).extension().is_some_and(|ext| ext == "rs") {
                            current_src = Some(
                                fs::read_to_string(&path).expect("read cited rust source file"),
                            );
                            current_file = Some(path);
                        } else {
                            current_src = None;
                            current_file = None;
                        }
                    } else if let Some(src) = &current_src {
                        let needle = format!("fn {token}(");
                        assert!(
                            src.contains(&needle),
                            "evidence register cites missing test `{token}` in {}",
                            current_file.as_ref().expect("file").display()
                        );
                    }
                }
            }
        }
    }
}
