//! Validation against the published JSON Schemas (mcp-plan.md sections 9.1 and 9.5, AINV11).
//!
//! The request and response decoders validate every document against its schema before typed
//! decoding, so the schemas and the decoders refuse the same inputs. The checks the decoders make
//! beyond the schemas are listed in mcp-plan.md section 9.1.
//!
//! The validator implements exactly the keywords the schema module emits; a test audits every
//! file for any other. `pattern` is evaluated by a small matcher for the regular-expression
//! subset the module writes: `^…$` anchors, literals, `[a-z]` classes, `(a|b)` groups and the
//! quantifiers `*`, `+`, `?`, `{n}` and `{m,n}`. It never calls a codec.
//!
//! Errors are deterministic: object keys are visited in byte order, and when no alternative of a
//! `oneOf` or `anyOf` matches, the reported failure is the deepest one, by number of pointer
//! segments, and the first on a tie. Integers compare exactly, also beyond 64 bits.

use std::cmp::Ordering;

use serde_json::Value;

/// Every keyword the validator implements. Annotation keywords are accepted and ignored.
pub const KEYWORDS: &[&str] = &[
    "$schema",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "prefixItems",
    "minItems",
    "maxItems",
    "uniqueItems",
    "enum",
    "const",
    "oneOf",
    "anyOf",
    "not",
    "minimum",
    "maximum",
    "pattern",
];

/// Where and why a document does not match its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// JSON Pointer into the document.
    pub path: String,
    pub message: String,
}

/// Validates `value` against the root of `schema`, a complete schema file.
pub fn validate(schema: &Value, value: &Value) -> Result<(), Violation> {
    let mut path = String::new();
    check(schema, schema, value, &mut path)
}

fn fail(path: &str, message: impl Into<String>) -> Result<(), Violation> {
    Err(Violation {
        path: path.to_owned(),
        message: message.into(),
    })
}

/// Escapes one JSON Pointer token.
pub(crate) fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn is_integer(v: &Value) -> bool {
    v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0)
}

/// A JSON number as an exact integer: 64-bit integers, and doubles with no fraction inside
/// the `i128` range (every such double is an exact integer).
fn exact(v: &Value) -> Option<i128> {
    const LIMIT: f64 = 1.7e38;
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
        .or_else(|| {
            let f = v.as_f64()?;
            // Every double of this size is an integer, so the cast is exact.
            (f.fract() == 0.0 && f.abs() < LIMIT).then_some(f as i128)
        })
}

/// Compares two JSON numbers exactly when both are integers, else as doubles.
fn num_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (exact(a), exact(b)) {
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        _ => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

/// The number of reference tokens in a JSON Pointer.
fn depth(path: &str) -> usize {
    path.matches('/').count()
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "integer" => v.is_number() && is_integer(v),
        "number" => v.is_number(),
        _ => false,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn check(root: &Value, schema: &Value, v: &Value, path: &mut String) -> Result<(), Violation> {
    let map = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return fail(path, "no value is allowed here"),
        Value::Object(map) => map,
        _ => return fail(path, "schema is not an object or a boolean"),
    };
    if let Some(r) = map.get("$ref").and_then(Value::as_str) {
        let target = r
            .strip_prefix("#/$defs/")
            .and_then(|name| root.get("$defs")?.get(name));
        match target {
            Some(target) => check(root, target, v, path)?,
            None => return fail(path, format!("unresolved reference {r}")),
        }
    }
    if let Some(ty) = map.get("type").and_then(Value::as_str)
        && !type_matches(ty, v)
    {
        return fail(path, format!("expected {ty}, found {}", type_name(v)));
    }
    if let Some(c) = map.get("const")
        && c != v
    {
        return fail(path, format!("expected {c}"));
    }
    if let Some(e) = map.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        return fail(path, "not one of the allowed values");
    }
    if v.is_number() {
        if let Some(min) = map.get("minimum")
            && num_cmp(v, min) == Some(Ordering::Less)
        {
            return fail(path, format!("below the minimum {min}"));
        }
        if let Some(max) = map.get("maximum")
            && num_cmp(v, max) == Some(Ordering::Greater)
        {
            return fail(path, format!("above the maximum {max}"));
        }
    }
    if let (Some(p), Some(s)) = (map.get("pattern").and_then(Value::as_str), v.as_str()) {
        match Pattern::parse(p) {
            Some(pattern) if pattern.is_match(s) => {}
            Some(_) => return fail(path, format!("does not match {p}")),
            None => return fail(path, format!("unsupported pattern {p}")),
        }
    }
    if let Some(obj) = v.as_object() {
        if let Some(required) = map.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !obj.contains_key(name) {
                    return fail(path, format!("missing field `{name}`"));
                }
            }
        }
        let props = map.get("properties").and_then(Value::as_object);
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        for key in keys {
            let len = path.len();
            path.push('/');
            path.push_str(&escape(key));
            let value = &obj[key.as_str()];
            let result = match props.and_then(|p| p.get(key.as_str())) {
                Some(s) => check(root, s, value, path),
                None => match map.get("additionalProperties") {
                    Some(Value::Bool(false)) => fail(path, "unknown field"),
                    Some(extra) => check(root, extra, value, path),
                    None => Ok(()),
                },
            };
            path.truncate(len);
            result?;
        }
    }
    if let Some(items) = v.as_array() {
        let len = items.len() as u64;
        if map
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| len > m)
        {
            return fail(path, "too many items");
        }
        if map
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| len < m)
        {
            return fail(path, "too few items");
        }
        let prefix = map
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for (i, item) in items.iter().enumerate() {
            if let Some(s) = prefix.get(i).or_else(|| map.get("items")) {
                let len = path.len();
                path.push('/');
                path.push_str(&i.to_string());
                let result = check(root, s, item, path);
                path.truncate(len);
                result?;
            }
        }
        if map.get("uniqueItems") == Some(&Value::Bool(true)) {
            for (i, a) in items.iter().enumerate() {
                if items[..i].contains(a) {
                    return fail(path, "repeats an item");
                }
            }
        }
    }
    if let Some(not) = map.get("not")
        && check(root, not, v, path).is_ok()
    {
        return fail(path, "matches a form that is not allowed");
    }
    if let Some(alternatives) = map.get("oneOf").and_then(Value::as_array) {
        let (passing, deepest) = alternatives_of(root, alternatives, v, path);
        match (passing, deepest) {
            (1, _) => {}
            (0, Some(deepest)) => return Err(deepest),
            _ => return fail(path, "matches more than one alternative"),
        }
    }
    if let Some(alternatives) = map.get("anyOf").and_then(Value::as_array) {
        let (passing, deepest) = alternatives_of(root, alternatives, v, path);
        if let (0, Some(deepest)) = (passing, deepest) {
            return Err(deepest);
        }
    }
    Ok(())
}

/// Counts the passing alternatives and keeps the deepest failure by segment count (the first
/// on a tie).
fn alternatives_of(
    root: &Value,
    alternatives: &[Value],
    v: &Value,
    path: &mut String,
) -> (usize, Option<Violation>) {
    let mut passing = 0;
    let mut deepest: Option<Violation> = None;
    for alternative in alternatives {
        match check(root, alternative, v, path) {
            Ok(()) => passing += 1,
            Err(e) => {
                if deepest
                    .as_ref()
                    .is_none_or(|d| depth(&e.path) > depth(&d.path))
                {
                    deepest = Some(e);
                }
            }
        }
    }
    (passing, deepest)
}

// ---- patterns ------------------------------------------------------------------------------

#[derive(Debug)]
enum Node {
    Char(char),
    Class(Vec<(char, char)>),
    Group(Vec<Vec<Node>>),
    Repeat(Box<Node>, u32, Option<u32>),
}

/// A compiled pattern of the supported subset, anchored at both ends.
#[derive(Debug)]
pub struct Pattern(Vec<Node>);

impl Pattern {
    /// Parses `^…$`; `None` for anything outside the subset.
    pub fn parse(pattern: &str) -> Option<Self> {
        let body = pattern.strip_prefix('^')?.strip_suffix('$')?;
        let chars: Vec<char> = body.chars().collect();
        let mut pos = 0;
        let mut alts = parse_alternatives(&chars, &mut pos)?;
        if pos != chars.len() {
            return None;
        }
        let nodes = if alts.len() == 1 {
            alts.pop()?
        } else {
            vec![Node::Group(alts)]
        };
        Some(Self(nodes))
    }

    /// Whether the whole of `s` matches.
    pub fn is_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        seq(&self.0, &chars, 0, &|end| end == chars.len())
    }
}

fn parse_alternatives(c: &[char], pos: &mut usize) -> Option<Vec<Vec<Node>>> {
    let mut alts = vec![parse_sequence(c, pos)?];
    while c.get(*pos) == Some(&'|') {
        *pos += 1;
        alts.push(parse_sequence(c, pos)?);
    }
    Some(alts)
}

fn parse_class(c: &[char], pos: &mut usize) -> Option<Node> {
    let mut ranges = Vec::new();
    while let Some(&lo) = c.get(*pos) {
        if lo == ']' {
            break;
        }
        if matches!(lo, '\\' | '[' | '^') {
            return None;
        }
        let hi = if c.get(*pos + 1) == Some(&'-') && c.get(*pos + 2).is_some_and(|&h| h != ']') {
            *pos += 2;
            *c.get(*pos)?
        } else {
            lo
        };
        *pos += 1;
        ranges.push((lo, hi));
    }
    if c.get(*pos) != Some(&']') || ranges.is_empty() {
        return None;
    }
    *pos += 1;
    Some(Node::Class(ranges))
}

fn parse_sequence(c: &[char], pos: &mut usize) -> Option<Vec<Node>> {
    let mut nodes = Vec::new();
    while let Some(&ch) = c.get(*pos) {
        let atom = match ch {
            '|' | ')' => break,
            '(' => {
                *pos += 1;
                let alts = parse_alternatives(c, pos)?;
                if c.get(*pos) != Some(&')') {
                    return None;
                }
                *pos += 1;
                Node::Group(alts)
            }
            '[' => {
                *pos += 1;
                parse_class(c, pos)?
            }
            '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '{' | '}' | ']' => return None,
            literal => {
                *pos += 1;
                Node::Char(literal)
            }
        };
        nodes.push(parse_quantifier(atom, c, pos)?);
    }
    Some(nodes)
}

fn parse_quantifier(atom: Node, c: &[char], pos: &mut usize) -> Option<Node> {
    let (min, max) = match c.get(*pos) {
        Some('*') => (0, None),
        Some('+') => (1, None),
        Some('?') => (0, Some(1)),
        Some('{') => {
            let close = c[*pos..].iter().position(|&ch| ch == '}')? + *pos;
            let inner: String = c[*pos + 1..close].iter().collect();
            let bounds = match inner.split_once(',') {
                Some((m, n)) => (m.parse().ok()?, Some(n.parse().ok()?)),
                None => {
                    let n = inner.parse().ok()?;
                    (n, Some(n))
                }
            };
            *pos = close;
            bounds
        }
        _ => return Some(atom),
    };
    *pos += 1;
    Some(Node::Repeat(Box::new(atom), min, max))
}

fn seq(nodes: &[Node], s: &[char], i: usize, k: &dyn Fn(usize) -> bool) -> bool {
    match nodes.split_first() {
        None => k(i),
        Some((first, rest)) => node(first, s, i, &|j| seq(rest, s, j, k)),
    }
}

fn node(n: &Node, s: &[char], i: usize, k: &dyn Fn(usize) -> bool) -> bool {
    match n {
        Node::Char(c) => s.get(i) == Some(c) && k(i + 1),
        Node::Class(ranges) => {
            s.get(i)
                .is_some_and(|ch| ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(ch)))
                && k(i + 1)
        }
        Node::Group(alts) => alts.iter().any(|alt| seq(alt, s, i, k)),
        Node::Repeat(inner, min, max) => repeat(inner, (*min, *max), 0, s, i, k),
    }
}

fn repeat(
    inner: &Node,
    (min, max): (u32, Option<u32>),
    count: u32,
    s: &[char],
    i: usize,
    k: &dyn Fn(usize) -> bool,
) -> bool {
    let more = max.is_none_or(|m| count < m)
        && node(inner, s, i, &|j| {
            j != i && repeat(inner, (min, max), count + 1, s, j, k)
        });
    more || (count >= min && k(i))
}

/// A pattern group matching the canonical decimal spelling of `0..=max`: no sign and no leading
/// zero. The id patterns use it, so a published schema refuses an id its codec cannot hold.
pub fn decimal_up_to(max: u64) -> String {
    let digits: Vec<u8> = max.to_string().bytes().map(|b| b - b'0').collect();
    let len = digits.len();
    let any = |n: usize| match n {
        0 => String::new(),
        1 => "[0-9]".to_owned(),
        n => format!("[0-9]{{{n}}}"),
    };
    let class = |lo: u8, hi: u8| {
        if lo == hi {
            lo.to_string()
        } else {
            format!("[{lo}-{hi}]")
        }
    };
    let mut alts = vec!["0".to_owned()];
    match len {
        0 | 1 => {}
        2 => alts.push("[1-9]".to_owned()),
        _ => alts.push(format!("[1-9][0-9]{{0,{}}}", len - 2)),
    }
    let mut prefix = String::new();
    for (i, &d) in digits.iter().enumerate() {
        let lo = u8::from(i == 0);
        if d > lo {
            alts.push(format!("{prefix}{}{}", class(lo, d - 1), any(len - i - 1)));
        }
        prefix.push(char::from(b'0' + d));
    }
    alts.push(prefix);
    alts.dedup();
    format!("({})", alts.join("|"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn integers_compare_exactly_beyond_64_bits() {
        let schema = json!({ "type": "integer", "minimum": 0, "maximum": u64::MAX });
        assert!(validate(&schema, &json!(u64::MAX)).is_ok());
        let over: Value = serde_json::from_str("18446744073709551616").unwrap();
        assert!(validate(&schema, &over).is_err());
        let minus_zero: Value = serde_json::from_str("-0").unwrap();
        assert!(validate(&schema, &minus_zero).is_ok());
        // The parsed double is compared exactly; a literal that a double parser rounds onto
        // the bound is the parser's loss, and the strict decoder refuses such literals anyway.
        let schema = json!({ "type": "integer", "minimum": i64::MIN });
        let under: Value = serde_json::from_str("-10000000000000000000").unwrap();
        assert!(validate(&schema, &under).is_err());
    }

    #[test]
    fn unions_report_the_failure_with_most_segments() {
        let schema = json!({ "oneOf": [
            { "type": "object", "properties": { "long_field": { "type": "string" } } },
            { "type": "object", "properties": { "a": { "type": "object",
                "properties": { "b": { "type": "string" } } } } }
        ] });
        let v = json!({ "long_field": 1, "a": { "b": 1 } });
        assert_eq!(validate(&schema, &v).unwrap_err().path, "/a/b");
    }

    #[test]
    fn patterns_match_the_regular_expression_subset() {
        let p = Pattern::parse("^pv-[0-9a-f]{16}$").unwrap();
        assert!(p.is_match("pv-5f0c1d2e3a4b5c6d"));
        assert!(!p.is_match("pv-5f0c1d2e3a4b5c6"));
        assert!(!p.is_match("pv-5F0C1D2E3A4B5C6D"));
        let p = Pattern::parse("^(part|wire):(0|[1-9][0-9]*)$").unwrap();
        assert!(p.is_match("wire:0") && p.is_match("part:120"));
        assert!(!p.is_match("part:012") && !p.is_match("label:1") && !p.is_match("part:"));
        assert!(Pattern::parse("^a.b$").is_none());
        assert!(Pattern::parse("a$").is_none());
    }

    #[test]
    fn decimal_ranges_are_exact_at_their_edges() {
        for max in [
            0,
            1,
            9,
            10,
            99,
            100,
            255,
            1_000,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let p = Pattern::parse(&format!("^{}$", decimal_up_to(max))).unwrap();
            for n in [
                0u64, 1, 9, 10, 11, 99, 100, 101, 255, 256, 999, 1_000, 1_001,
            ] {
                assert_eq!(p.is_match(&n.to_string()), n <= max, "{n} against {max}");
            }
            assert!(p.is_match(&max.to_string()));
            match max.checked_add(1) {
                Some(next) => assert!(!p.is_match(&next.to_string()), "{next} against {max}"),
                None => assert!(!p.is_match("18446744073709551616")),
            }
            assert!(!p.is_match("01") && !p.is_match("-1") && !p.is_match(""));
        }
    }
}
