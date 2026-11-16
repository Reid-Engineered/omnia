//! `cargo xtask tokens`: generate `crates/omnia-theme/src/generated.rs` (plan section 9).
//!
//! Inputs are the `Theme` variable collection of `omnia.figcraft` (modes `Graphite` and
//! `Gray`) and the overlay `crates/omnia-theme/canvas-tokens.toml` (plan Q2). A token present
//! in both is taken from `omnia.figcraft`. Output is sorted by token name and formatted the
//! way rustfmt formats it, so running the task twice gives no diff.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use serde_json::Value;

/// The `Theme` collection modes, in output order, with the static each one becomes.
const MODES: [(&str, &str); 2] = [("Graphite", "GRAPHITE"), ("Gray", "GRAY")];

pub const FIGCRAFT: &str = "omnia.figcraft";
pub const OVERLAY: &str = "crates/omnia-theme/canvas-tokens.toml";
pub const GENERATED: &str = "crates/omnia-theme/src/generated.rs";

type Error = String;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Color([f32; 4]);

/// Where a token's colours came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Figcraft,
    Overlay,
}

#[derive(Debug)]
struct Token {
    source: Source,
    /// One colour per entry of `MODES`.
    values: [Color; 2],
}

/// Runs the task against the workspace at `root`.
pub fn run(root: &Path) -> Result<(), Error> {
    let out = render_from_files(root)?;
    let path = root.join(GENERATED);
    // A checkout with CRLF line endings holds the same tokens.
    let unchanged = fs::read_to_string(&path).is_ok_and(|old| old.replace("\r\n", "\n") == out);
    if unchanged {
        println!("tokens: {GENERATED} is up to date");
    } else {
        fs::write(&path, out).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!("tokens: wrote {GENERATED}");
    }
    Ok(())
}

/// Reads both inputs under `root` and returns the text of `generated.rs`.
pub fn render_from_files(root: &Path) -> Result<String, Error> {
    let document = read_figcraft_document(&root.join(FIGCRAFT))?;
    let overlay_path = root.join(OVERLAY);
    let overlay = fs::read_to_string(&overlay_path)
        .map_err(|e| format!("read {}: {e}", overlay_path.display()))?;
    let tokens = collect(&document, &overlay)?;
    Ok(render(&tokens))
}

/// `omnia.figcraft` is a zip archive; the document is `document.json` inside it.
fn read_figcraft_document(path: &Path) -> Result<Value, Error> {
    let file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{} is not a zip: {e}", path.display()))?;
    let mut entry = archive
        .by_name("document.json")
        .map_err(|e| format!("{}: document.json: {e}", path.display()))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("{}: read document.json: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: parse document.json: {e}", path.display()))
}

/// Merges the figcraft `Theme` colours and the overlay into one sorted token map.
fn collect(document: &Value, overlay: &str) -> Result<BTreeMap<String, Token>, Error> {
    let mut tokens = figcraft_tokens(document)?;
    let overlay = overlay_entries(overlay)?;
    for name in overlay.keys() {
        if tokens.contains_key(name) {
            println!("tokens: `{name}` is in {FIGCRAFT}; its overlay entry is ignored");
        }
    }
    for name in overlay.keys() {
        if !tokens.contains_key(name) {
            let mut stack = Vec::new();
            let values = resolve(name, &tokens, &overlay, &mut stack)?;
            tokens.insert(
                name.clone(),
                Token {
                    source: Source::Overlay,
                    values,
                },
            );
        }
    }
    let mut fields = BTreeMap::new();
    for name in tokens.keys() {
        if let Some(other) = fields.insert(field_name(name)?, name) {
            return Err(format!(
                "tokens `{other}` and `{name}` map to the same field"
            ));
        }
    }
    Ok(tokens)
}

/// The colour variables of the `Theme` collection, one value per mode in `MODES`.
fn figcraft_tokens(document: &Value) -> Result<BTreeMap<String, Token>, Error> {
    let variables = &document["variables"];
    let collections = variables["collections"]
        .as_array()
        .ok_or("document.json: no variables.collections array")?;
    let theme = collections
        .iter()
        .find(|c| c["name"] == "Theme")
        .ok_or("document.json: no variable collection named `Theme`")?;
    let collection_id = theme["id"].as_str().ok_or("Theme collection has no id")?;
    let modes = theme["modes"]
        .as_array()
        .ok_or("Theme collection has no modes")?;
    let mut mode_ids = Vec::new();
    for (mode, _) in MODES {
        let id = modes
            .iter()
            .find(|m| m["name"] == mode)
            .and_then(|m| m["id"].as_str())
            .ok_or_else(|| format!("Theme collection has no mode `{mode}`"))?;
        mode_ids.push(id);
    }

    let mut tokens = BTreeMap::new();
    let list = variables["variables"]
        .as_array()
        .ok_or("document.json: no variables.variables array")?;
    for variable in list {
        if variable["collection"] != collection_id || variable["resolvedType"] != "color" {
            continue;
        }
        let name = variable["name"]
            .as_str()
            .ok_or("a Theme variable has no name")?;
        let mut values = [Color([0.0; 4]); 2];
        for (slot, mode_id) in values.iter_mut().zip(&mode_ids) {
            let value = &variable["values"][*mode_id];
            if value["type"] != "color" {
                return Err(format!(
                    "`{name}` in mode {mode_id} is not a plain colour: {value}"
                ));
            }
            let channel = |key: &str| {
                value["value"][key]
                    .as_f64()
                    .map(|v| v as f32)
                    .ok_or_else(|| format!("`{name}` in mode {mode_id} has no channel `{key}`"))
            };
            *slot = Color([channel("r")?, channel("g")?, channel("b")?, channel("a")?]);
        }
        let token = Token {
            source: Source::Figcraft,
            values,
        };
        if tokens.insert(name.to_owned(), token).is_some() {
            return Err(format!("Theme collection has two variables named `{name}`"));
        }
    }
    Ok(tokens)
}

/// The overlay's `[tokens]` table: name to value string.
fn overlay_entries(text: &str) -> Result<BTreeMap<String, String>, Error> {
    let table: toml::Table = text.parse().map_err(|e| format!("{OVERLAY}: {e}"))?;
    let tokens = table
        .get("tokens")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{OVERLAY}: no [tokens] table"))?;
    let mut entries = BTreeMap::new();
    for (name, value) in tokens {
        let value = value
            .as_str()
            .ok_or_else(|| format!("{OVERLAY}: `{name}` is not a string"))?;
        entries.insert(name.clone(), value.to_owned());
    }
    Ok(entries)
}

/// Resolves an overlay entry to one colour per mode. A hex value is the same in every mode;
/// a reference takes the referenced token's value in each mode.
fn resolve(
    name: &str,
    tokens: &BTreeMap<String, Token>,
    overlay: &BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<[Color; 2], Error> {
    if let Some(token) = tokens.get(name) {
        return Ok(token.values);
    }
    let value = overlay
        .get(name)
        .ok_or_else(|| format!("{OVERLAY}: unknown token `{name}` (referenced by {stack:?})"))?;
    if stack.iter().any(|s| s == name) {
        return Err(format!("{OVERLAY}: reference cycle {stack:?} -> {name}"));
    }
    if let Some(hex) = value.strip_prefix('#') {
        let color = parse_hex(hex).ok_or_else(|| format!("{OVERLAY}: `{name}` = {value:?}"))?;
        return Ok([color; 2]);
    }
    stack.push(name.to_owned());
    let values = resolve(value, tokens, overlay, stack)?;
    stack.pop();
    Ok(values)
}

/// `RRGGBB` or `RRGGBBAA`.
fn parse_hex(hex: &str) -> Option<Color> {
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
    let channels = [byte(0)?, byte(2)?, byte(4)?, alpha];
    Some(Color(channels.map(|c| f32::from(c) / 255.0)))
}

/// `stage/ink-secondary` becomes `stage_ink_secondary`.
fn field_name(token: &str) -> Result<String, Error> {
    let field: String = token
        .chars()
        .map(|c| if c == '/' || c == '-' { '_' } else { c })
        .collect();
    let valid = field.starts_with(|c: char| c.is_ascii_lowercase())
        && field
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(field)
    } else {
        Err(format!("token `{token}` does not map to a Rust field name"))
    }
}

fn render(tokens: &BTreeMap<String, Token>) -> String {
    let count = tokens.len();
    let figcraft = tokens
        .values()
        .filter(|t| t.source == Source::Figcraft)
        .count();
    let overlay = count - figcraft;
    let mut out = String::new();
    let w = &mut out;
    // `write!` into a String cannot fail.
    let _ = writeln!(
        w,
        "// @generated by `cargo xtask tokens` from {FIGCRAFT} (`Theme` collection)"
    );
    let _ = writeln!(w, "// and {OVERLAY}. Do not edit by hand.");
    let _ = writeln!(w);
    let _ = writeln!(w, "use crate::Rgba;");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "/// One field per colour token: {figcraft} from `{FIGCRAFT}`, {overlay} from the overlay."
    );
    let _ = writeln!(w, "#[derive(Clone, Copy, Debug, PartialEq)]");
    let _ = writeln!(w, "pub struct Theme {{");
    for (name, token) in tokens {
        let from = match token.source {
            Source::Figcraft => FIGCRAFT,
            Source::Overlay => "canvas-tokens.toml",
        };
        let _ = writeln!(w, "    /// `{name}` ({from}).");
        let _ = writeln!(w, "    pub {}: Rgba,", field_name(name).unwrap_or_default());
    }
    let _ = writeln!(w, "}}");
    let _ = writeln!(w);
    let _ = writeln!(w, "impl Theme {{");
    let _ = writeln!(w, "    /// The number of colour tokens.");
    let _ = writeln!(w, "    pub const TOKEN_COUNT: usize = {count};");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "    /// Every token as `(name, colour)`, sorted by name."
    );
    let _ = writeln!(
        w,
        "    pub fn tokens(&self) -> [(&'static str, Rgba); Self::TOKEN_COUNT] {{"
    );
    let _ = writeln!(w, "        [");
    for name in tokens.keys() {
        let field = field_name(name).unwrap_or_default();
        let _ = writeln!(w, "            ({name:?}, self.{field}),");
    }
    let _ = writeln!(w, "        ]");
    let _ = writeln!(w, "    }}");
    let _ = writeln!(w, "}}");
    for (index, (mode, ident)) in MODES.iter().enumerate() {
        let _ = writeln!(w);
        let _ = writeln!(w, "/// The `{mode}` mode of the `Theme` collection.");
        let _ = writeln!(w, "pub static {ident}: Theme = Theme {{");
        for (name, token) in tokens {
            let field = field_name(name).unwrap_or_default();
            let [r, g, b, a] = token.values[index].0;
            let _ = writeln!(w, "    {field}: Rgba::new({r:?}, {g:?}, {b:?}, {a:?}),");
        }
        let _ = writeln!(w, "}};");
    }
    out
}

/// The workspace root: the parent of the `xtask` package.
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn generated_rs_is_up_to_date() {
        let root = workspace_root();
        let expected = render_from_files(&root).expect("render tokens");
        let actual = fs::read_to_string(root.join(GENERATED)).expect("read generated.rs");
        // A checkout with CRLF line endings holds the same tokens.
        let actual = actual.replace("\r\n", "\n");
        assert!(
            expected == actual,
            "{GENERATED} is stale; run `cargo xtask tokens`"
        );
    }

    #[test]
    fn every_figcraft_colour_and_overlay_token_is_generated() {
        let root = workspace_root();
        let document = read_figcraft_document(&root.join(FIGCRAFT)).expect("read figcraft");
        let overlay = fs::read_to_string(root.join(OVERLAY)).expect("read overlay");
        let figcraft: BTreeSet<String> = figcraft_tokens(&document).unwrap().into_keys().collect();
        let overlay_names: BTreeSet<String> =
            overlay_entries(&overlay).unwrap().into_keys().collect();
        let generated: BTreeSet<String> =
            collect(&document, &overlay).unwrap().into_keys().collect();
        let expected: BTreeSet<String> = figcraft.union(&overlay_names).cloned().collect();
        assert_eq!(generated, expected);
    }

    #[test]
    fn overlay_references_resolve_per_mode_and_figcraft_wins() {
        let document = serde_json::json!({
            "variables": {
                "collections": [{
                    "id": "C1",
                    "name": "Theme",
                    "modes": [{"id": "m1", "name": "Graphite"}, {"id": "m2", "name": "Gray"}]
                }],
                "variables": [{
                    "name": "accent/mint",
                    "collection": "C1",
                    "resolvedType": "color",
                    "values": {
                        "m1": {"type": "color", "value": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}},
                        "m2": {"type": "color", "value": {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0}}
                    }
                }]
            }
        });
        let overlay = r##"
            [tokens]
            "accent/mint" = "#000000"
            "stage/snap" = "accent/mint"
            "stage/grid" = "#00000022"
            "stage/a" = "stage/b"
            "stage/b" = "#FFFFFF"
        "##;
        let tokens = collect(&document, overlay).unwrap();
        assert_eq!(tokens["accent/mint"].source, Source::Figcraft);
        assert_eq!(tokens["stage/snap"].values, tokens["accent/mint"].values);
        assert_eq!(tokens["stage/a"].values, [Color([1.0; 4]); 2]);
        let grid = tokens["stage/grid"].values[0].0;
        assert_eq!(grid, [0.0, 0.0, 0.0, f32::from(0x22u8) / 255.0]);

        let cycle = "[tokens]\n\"x/a\" = \"x/b\"\n\"x/b\" = \"x/a\"\n";
        assert!(collect(&document, cycle).unwrap_err().contains("cycle"));
        let unknown = "[tokens]\n\"x/a\" = \"x/missing\"\n";
        assert!(
            collect(&document, unknown)
                .unwrap_err()
                .contains("unknown token")
        );
        let bad_hex = "[tokens]\n\"x/a\" = \"#12345\"\n";
        assert!(collect(&document, bad_hex).is_err());
    }
}
