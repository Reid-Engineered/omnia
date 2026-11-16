//! `cargo xtask symbols`: compile vendored SVGs + `pins.toml` into `crates/omnia-symbols/src/generated.rs`.
//!
//! Reads `crates/omnia-symbols/pins.toml` and the referenced SVGs under
//! `vendor/electronic-symbols/SVG/`, verifies every pin lands on the 1.27 mm
//! (`PIN_LATTICE_NM`) lattice (K2) and within K1 bounds, snaps stroked lead ends
//! to the declared pin coordinates in the 150-unit box, and writes deterministic,
//! rustfmt-clean Rust statics.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

use omnia_doc::{GRID_NM, PIN_LATTICE_NM, Pt, Rect};
use usvg::tiny_skia_path::{self, PathSegment};

pub const PINS_TOML: &str = "crates/omnia-symbols/pins.toml";
pub const VENDOR_SVG_DIR: &str = "vendor/electronic-symbols/SVG";
pub const GENERATED: &str = "crates/omnia-symbols/src/generated.rs";

/// K7: vendor symbols are drawn in a 150-unit square viewBox.
const VIEW_BOX_UNITS: f64 = 150.0;
/// K3: 20 logical px per grid step (2,540,000 nm) -> 127,000 nm per 100%-zoom px.
const NM_PER_PX: i64 = GRID_NM as i64 / 20;
/// Maximum distance (in 150-box units) from a declared pin for a lead end to snap.
const LEAD_SNAP_RADIUS: f32 = 1.0;
/// Tolerance (in 150-box units) for treating a lead segment as axis-aligned.
const AXIS_EPS: f32 = 1e-2;

type Error = String;

#[derive(Debug, Clone)]
struct PinSpec {
    number: String,
    at: [f64; 2],
}

#[derive(Debug, Clone)]
struct SymbolSpec {
    size_px: f64,
    anchor: [f64; 2],
    pins: Vec<PinSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone)]
struct CompiledPin {
    number: String,
    svg_at: [f32; 2],
    world_at: Pt,
}

#[derive(Debug, Clone)]
struct CompiledPath {
    d: String,
    fill: bool,
    stroke: bool,
    linecap: LineCap,
    linejoin: LineJoin,
    miterlimit: f32,
}

#[derive(Debug, Clone)]
struct CompiledSymbol {
    name: String,
    bounds: Rect,
    pins: Vec<CompiledPin>,
    original_stroke_units: f32,
    paths: Vec<CompiledPath>,
}

/// Runs `cargo xtask symbols` against the workspace at `root`.
pub fn run(root: &Path) -> Result<(), Error> {
    let out = render_from_files(root)?;
    let path = root.join(GENERATED);
    let unchanged = fs::read_to_string(&path).is_ok_and(|old| old.replace("\r\n", "\n") == out);
    if unchanged {
        println!("symbols: {GENERATED} is up to date");
    } else {
        fs::write(&path, out).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!("symbols: wrote {GENERATED}");
    }
    Ok(())
}

/// Reads `pins.toml` and vendor SVGs under `root`, and renders `generated.rs`.
pub fn render_from_files(root: &Path) -> Result<String, Error> {
    let pins_path = root.join(PINS_TOML);
    let pins_text =
        fs::read_to_string(&pins_path).map_err(|e| format!("read {}: {e}", pins_path.display()))?;
    let specs = parse_pins_toml(&pins_text)?;
    let svg_dir = root.join(VENDOR_SVG_DIR);
    let compiled = compile_all(&specs, &svg_dir)?;
    Ok(render_rust(&compiled))
}

fn parse_pins_toml(text: &str) -> Result<BTreeMap<String, SymbolSpec>, Error> {
    let table: toml::Table = text.parse().map_err(|e| format!("{PINS_TOML}: {e}"))?;
    let symbols = table
        .get("symbols")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{PINS_TOML}: missing [symbols] table"))?;

    let mut out = BTreeMap::new();
    for (name, val) in symbols {
        let entry = val
            .as_table()
            .ok_or_else(|| format!("{PINS_TOML}: symbols.{name} must be a table"))?;
        let size_px = number_field(entry.get("size_px"), &format!("symbols.{name}.size_px"))?;
        if !size_px.is_finite() || size_px <= 0.0 {
            return Err(format!(
                "{PINS_TOML}: symbols.{name}.size_px must be finite and > 0"
            ));
        }
        let anchor = match entry.get("anchor") {
            Some(v) => point_field(v, &format!("symbols.{name}.anchor"))?,
            None => [75.0, 75.0],
        };
        let pins_val = entry
            .get("pins")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("{PINS_TOML}: symbols.{name}.pins must be an array"))?;
        if pins_val.is_empty() {
            return Err(format!("{PINS_TOML}: symbols.{name}.pins cannot be empty"));
        }
        let mut pins = Vec::with_capacity(pins_val.len());
        let mut seen_numbers = BTreeSet::new();
        for (idx, pin_val) in pins_val.iter().enumerate() {
            let pin_tbl = pin_val.as_table().ok_or_else(|| {
                format!("{PINS_TOML}: symbols.{name}.pins[{idx}] must be a table")
            })?;
            let number = pin_tbl
                .get("number")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    format!("{PINS_TOML}: symbols.{name}.pins[{idx}].number must be a string")
                })?;
            if number.is_empty() {
                return Err(format!(
                    "{PINS_TOML}: symbols.{name}.pins[{idx}].number cannot be empty"
                ));
            }
            if !seen_numbers.insert(number.to_owned()) {
                return Err(format!(
                    "{PINS_TOML}: symbols.{name} has duplicate pin number `{number}`"
                ));
            }
            let at = pin_tbl
                .get("at")
                .ok_or_else(|| format!("{PINS_TOML}: symbols.{name}.pins[{idx}] missing `at`"))?;
            let at = point_field(at, &format!("symbols.{name}.pins[{idx}].at"))?;
            pins.push(PinSpec {
                number: number.to_owned(),
                at,
            });
        }
        out.insert(
            name.clone(),
            SymbolSpec {
                size_px,
                anchor,
                pins,
            },
        );
    }
    if out.is_empty() {
        return Err(format!("{PINS_TOML}: no symbols defined"));
    }
    Ok(out)
}

fn number_field(val: Option<&toml::Value>, ctx: &str) -> Result<f64, Error> {
    let val = val.ok_or_else(|| format!("{PINS_TOML}: missing `{ctx}`"))?;
    match val {
        toml::Value::Integer(i) => Ok(*i as f64),
        toml::Value::Float(f) => Ok(*f),
        _ => Err(format!("{PINS_TOML}: `{ctx}` must be a number")),
    }
}

fn point_field(val: &toml::Value, ctx: &str) -> Result<[f64; 2], Error> {
    let arr = val
        .as_array()
        .ok_or_else(|| format!("{PINS_TOML}: `{ctx}` must be a 2-element array"))?;
    if arr.len() != 2 {
        return Err(format!("{PINS_TOML}: `{ctx}` must have 2 elements"));
    }
    let x = number_field(Some(&arr[0]), &format!("{ctx}[0]"))?;
    let y = number_field(Some(&arr[1]), &format!("{ctx}[1]"))?;
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("{PINS_TOML}: `{ctx}` coordinates must be finite"));
    }
    Ok([x, y])
}

fn compile_all(
    specs: &BTreeMap<String, SymbolSpec>,
    svg_dir: &Path,
) -> Result<Vec<CompiledSymbol>, Error> {
    let mut out = Vec::with_capacity(specs.len());
    for (name, spec) in specs {
        let svg_path = svg_dir.join(format!("{name}.svg"));
        let svg_bytes =
            fs::read(&svg_path).map_err(|e| format!("read {}: {e}", svg_path.display()))?;
        out.push(compile_symbol(name, spec, &svg_bytes)?);
    }
    Ok(out)
}

fn compile_symbol(
    name: &str,
    spec: &SymbolSpec,
    svg_bytes: &[u8],
) -> Result<CompiledSymbol, Error> {
    let size_nm_f = spec.size_px * (NM_PER_PX as f64);
    let size_nm = size_nm_f.round() as i64;
    if (size_nm_f - size_nm as f64).abs() > 1e-3 || size_nm <= 0 {
        return Err(format!(
            "symbol `{name}`: size_px {} does not map to an integer nanometre size",
            spec.size_px
        ));
    }

    let box_pt = |svg_x: f64, svg_y: f64, label: &str| -> Result<Pt, Error> {
        let wx_f = (svg_x - spec.anchor[0]) * (size_nm as f64) / VIEW_BOX_UNITS;
        let wy_f = (svg_y - spec.anchor[1]) * (size_nm as f64) / VIEW_BOX_UNITS;
        let wx_r = wx_f.round();
        let wy_r = wy_f.round();
        if (wx_f - wx_r).abs() > 1e-3 || (wy_f - wy_r).abs() > 1e-3 {
            return Err(format!(
                "symbol `{name}` {label}: ({svg_x}, {svg_y}) maps to non-integer nm ({wx_f}, {wy_f})"
            ));
        }
        let wx = i32::try_from(wx_r as i64)
            .map_err(|_| format!("symbol `{name}` {label}: x overflows i32"))?;
        let wy = i32::try_from(wy_r as i64)
            .map_err(|_| format!("symbol `{name}` {label}: y overflows i32"))?;
        Pt::new(wx, wy).map_err(|e| format!("symbol `{name}` {label}: {e}"))
    };

    let min = box_pt(0.0, 0.0, "bounds min")?;
    let max = box_pt(VIEW_BOX_UNITS, VIEW_BOX_UNITS, "bounds max")?;
    let bounds = Rect::new(min, max).map_err(|e| format!("symbol `{name}` bounds: {e}"))?;

    let mut pins = Vec::with_capacity(spec.pins.len());
    for pin in &spec.pins {
        let world_at = box_pt(pin.at[0], pin.at[1], &format!("pin `{}`", pin.number))?;
        if world_at.x % PIN_LATTICE_NM != 0 || world_at.y % PIN_LATTICE_NM != 0 {
            return Err(format!(
                "symbol `{name}` pin `{}` at ({}, {}) -> ({}, {}) nm is off the {:.2} mm ({PIN_LATTICE_NM} nm) pin lattice",
                pin.number,
                pin.at[0],
                pin.at[1],
                world_at.x,
                world_at.y,
                PIN_LATTICE_NM as f64 / 1_000_000.0
            ));
        }
        pins.push(CompiledPin {
            number: pin.number.clone(),
            svg_at: [pin.at[0] as f32, pin.at[1] as f32],
            world_at,
        });
    }

    let pin_coords: Vec<[f32; 2]> = pins.iter().map(|p| p.svg_at).collect();
    let (original_stroke_units, paths) = extract_svg_paths(name, svg_bytes, &pin_coords)?;

    Ok(CompiledSymbol {
        name: name.to_owned(),
        bounds,
        pins,
        original_stroke_units,
        paths,
    })
}

fn extract_svg_paths(
    name: &str,
    svg_bytes: &[u8],
    pins: &[[f32; 2]],
) -> Result<(f32, Vec<CompiledPath>), Error> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg_bytes, &opts)
        .map_err(|e| format!("symbol `{name}`: parse SVG with usvg: {e}"))?;
    let sz = tree.size();
    if (sz.width() - VIEW_BOX_UNITS as f32).abs() > 1e-3
        || (sz.height() - VIEW_BOX_UNITS as f32).abs() > 1e-3
    {
        return Err(format!(
            "symbol `{name}`: expected {VIEW_BOX_UNITS}x{VIEW_BOX_UNITS} box, got {}x{}",
            sz.width(),
            sz.height()
        ));
    }
    if tree.has_defs_nodes() {
        return Err(format!(
            "symbol `{name}`: unsupported <defs> nodes (gradients/patterns/clips/masks/filters)"
        ));
    }

    let mut paths = Vec::new();
    collect_group(name, tree.root(), pins, &mut paths)?;
    if paths.is_empty() {
        return Err(format!("symbol `{name}`: SVG has no visible paths"));
    }

    let mut original_stroke: Option<f32> = None;
    for path in &paths {
        if let Some(w) = path.stroke_width {
            match original_stroke {
                None => original_stroke = Some(w),
                Some(prev) if (prev - w).abs() <= 1e-3 => {}
                Some(prev) => {
                    return Err(format!(
                        "symbol `{name}`: mixed stroke widths ({prev} vs {w})"
                    ));
                }
            }
        }
    }

    let compiled_paths = paths.into_iter().map(|p| p.path).collect();
    Ok((original_stroke.unwrap_or(5.0), compiled_paths))
}

struct ExtractedPath {
    path: CompiledPath,
    stroke_width: Option<f32>,
}

fn collect_group(
    name: &str,
    group: &usvg::Group,
    pins: &[[f32; 2]],
    out: &mut Vec<ExtractedPath>,
) -> Result<(), Error> {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect_group(name, g, pins, out)?,
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let fill = p.fill().is_some_and(|f| f.opacity().get() > 0.0);
                let stroke = p.stroke().filter(|s| s.opacity().get() > 0.0);
                if !fill && stroke.is_none() {
                    continue;
                }

                let abs_path = if p.abs_transform().is_identity() {
                    p.data().clone()
                } else {
                    p.data()
                        .clone()
                        .transform(p.abs_transform())
                        .ok_or_else(|| format!("symbol `{name}`: path transform failed"))?
                };

                let mut segs: Vec<PathSegment> = abs_path.segments().collect();
                if stroke.is_some() && !pins.is_empty() {
                    snap_lead_ends(&mut segs, pins);
                }
                let d = format_segments(&segs);
                if d.is_empty() {
                    continue;
                }

                let (linecap, linejoin, miterlimit, stroke_width) = match stroke {
                    Some(s) => (
                        match s.linecap() {
                            usvg::LineCap::Butt => LineCap::Butt,
                            usvg::LineCap::Round => LineCap::Round,
                            usvg::LineCap::Square => LineCap::Square,
                        },
                        match s.linejoin() {
                            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => LineJoin::Miter,
                            usvg::LineJoin::Round => LineJoin::Round,
                            usvg::LineJoin::Bevel => LineJoin::Bevel,
                        },
                        s.miterlimit().get(),
                        Some(s.width().get()),
                    ),
                    None => (LineCap::Butt, LineJoin::Miter, 4.0, None),
                };

                out.push(ExtractedPath {
                    path: CompiledPath {
                        d,
                        fill,
                        stroke: stroke.is_some(),
                        linecap,
                        linejoin,
                        miterlimit,
                    },
                    stroke_width,
                });
            }
            usvg::Node::Image(_) => {
                return Err(format!("symbol `{name}`: raster <image> is not supported"));
            }
            usvg::Node::Text(_) => {
                return Err(format!("symbol `{name}`: SVG <text> is not supported"));
            }
        }
    }
    Ok(())
}

/// Snaps open stroked subpath endpoints within `LEAD_SNAP_RADIUS` of a declared pin to
/// the exact pin coordinate, keeping axis-aligned lead stubs horizontal or vertical.
fn snap_lead_ends(segs: &mut [PathSegment], pins: &[[f32; 2]]) {
    let mut start = 0;
    while start < segs.len() {
        if !matches!(segs[start], PathSegment::MoveTo(_)) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < segs.len() && !matches!(segs[end], PathSegment::MoveTo(_)) {
            end += 1;
        }
        snap_subpath(&mut segs[start..end], pins);
        start = end;
    }
}

fn snap_subpath(sub: &mut [PathSegment], pins: &[[f32; 2]]) {
    if sub.len() < 2 {
        return;
    }
    if matches!(sub.last(), Some(PathSegment::Close)) {
        return;
    }

    // Snap the subpath start (MoveTo) if it lies near a pin.
    if let PathSegment::MoveTo(p0) = sub[0]
        && let Some([px, py]) = nearest_pin(p0, pins)
    {
        if let PathSegment::LineTo(ref mut p1) = sub[1] {
            if (p1.y - p0.y).abs() <= AXIS_EPS {
                p1.y = py;
            } else if (p1.x - p0.x).abs() <= AXIS_EPS {
                p1.x = px;
            }
        }
        sub[0] = PathSegment::MoveTo(tiny_skia_path::Point::from_xy(px, py));
    }

    // Snap the subpath end if it is a LineTo/QuadTo/CubicTo ending near a pin.
    let last_idx = sub.len() - 1;
    let prev_pt = segment_end(sub[last_idx - 1]);
    let end_pt = segment_end(sub[last_idx]);
    if let (Some(p_prev), Some(p_end)) = (prev_pt, end_pt)
        && let Some([px, py]) = nearest_pin(p_end, pins)
    {
        if matches!(sub[last_idx], PathSegment::LineTo(_)) {
            if (p_end.y - p_prev.y).abs() <= AXIS_EPS {
                set_segment_end(&mut sub[last_idx - 1], p_prev.x, py);
            } else if (p_end.x - p_prev.x).abs() <= AXIS_EPS {
                set_segment_end(&mut sub[last_idx - 1], px, p_prev.y);
            }
        }
        set_segment_end(&mut sub[last_idx], px, py);
    }
}

fn nearest_pin(pt: tiny_skia_path::Point, pins: &[[f32; 2]]) -> Option<[f32; 2]> {
    let mut best: Option<(f32, [f32; 2])> = None;
    for &[px, py] in pins {
        let dx = pt.x - px;
        let dy = pt.y - py;
        let dist2 = dx * dx + dy * dy;
        if dist2 <= LEAD_SNAP_RADIUS * LEAD_SNAP_RADIUS && best.is_none_or(|(b, _)| dist2 < b) {
            best = Some((dist2, [px, py]));
        }
    }
    best.map(|(_, p)| p)
}

fn segment_end(seg: PathSegment) -> Option<tiny_skia_path::Point> {
    match seg {
        PathSegment::MoveTo(p)
        | PathSegment::LineTo(p)
        | PathSegment::QuadTo(_, p)
        | PathSegment::CubicTo(_, _, p) => Some(p),
        PathSegment::Close => None,
    }
}

fn set_segment_end(seg: &mut PathSegment, x: f32, y: f32) {
    let pt = tiny_skia_path::Point::from_xy(x, y);
    match seg {
        PathSegment::MoveTo(p)
        | PathSegment::LineTo(p)
        | PathSegment::QuadTo(_, p)
        | PathSegment::CubicTo(_, _, p) => *p = pt,
        PathSegment::Close => {}
    }
}

fn format_segments(segs: &[PathSegment]) -> String {
    let mut out = String::new();
    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        match *seg {
            PathSegment::MoveTo(p) => {
                let _ = write!(out, "M {} {}", fmt_coord(p.x), fmt_coord(p.y));
            }
            PathSegment::LineTo(p) => {
                let _ = write!(out, "L {} {}", fmt_coord(p.x), fmt_coord(p.y));
            }
            PathSegment::QuadTo(p1, p) => {
                let _ = write!(
                    out,
                    "Q {} {} {} {}",
                    fmt_coord(p1.x),
                    fmt_coord(p1.y),
                    fmt_coord(p.x),
                    fmt_coord(p.y)
                );
            }
            PathSegment::CubicTo(p1, p2, p) => {
                let _ = write!(
                    out,
                    "C {} {} {} {} {} {}",
                    fmt_coord(p1.x),
                    fmt_coord(p1.y),
                    fmt_coord(p2.x),
                    fmt_coord(p2.y),
                    fmt_coord(p.x),
                    fmt_coord(p.y)
                );
            }
            PathSegment::Close => out.push('Z'),
        }
    }
    out
}

fn fmt_coord(v: f32) -> String {
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let clean = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{clean}")
}

fn fmt_pt(pt: Pt, indent: &str) -> String {
    let sx = fmt_i32(pt.x);
    let sy = fmt_i32(pt.y);
    let inline = format!("x: {sx}, y: {sy}");
    if inline.len() <= 18 {
        format!("Pt {{ {inline} }}")
    } else {
        format!("Pt {{\n{indent}    x: {sx},\n{indent}    y: {sy},\n{indent}}}")
    }
}

fn render_rust(symbols: &[CompiledSymbol]) -> String {
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(
        w,
        "// @generated by `cargo xtask symbols` from {PINS_TOML} and {VENDOR_SVG_DIR}/."
    );
    let _ = writeln!(w, "// Do not edit by hand.");
    let _ = writeln!(w);
    let _ = writeln!(w, "use omnia_doc::{{Pt, Rect}};");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "use crate::{{ArtPath, LineCap, LineJoin, Pin, SymbolArt, SymbolDef, SymbolId}};"
    );
    let _ = writeln!(w);

    for (idx, sym) in symbols.iter().enumerate() {
        let ident = const_ident(&sym.name);
        if sym.pins.len() == 1 {
            let pin = &sym.pins[0];
            let _ = writeln!(w, "static {ident}_PINS: &[Pin] = &[Pin {{");
            let _ = writeln!(w, "    at: {},", fmt_pt(pin.world_at, "    "));
            let _ = writeln!(w, "    number: {:?},", pin.number);
            let _ = writeln!(w, "}}];");
        } else {
            let _ = writeln!(w, "static {ident}_PINS: &[Pin] = &[");
            for pin in &sym.pins {
                let _ = writeln!(w, "    Pin {{");
                let _ = writeln!(w, "        at: {},", fmt_pt(pin.world_at, "        "));
                let _ = writeln!(w, "        number: {:?},", pin.number);
                let _ = writeln!(w, "    }},");
            }
            let _ = writeln!(w, "];");
        }
        let _ = writeln!(w);

        let write_path_fields = |w: &mut String, path: &CompiledPath, indent: &str| {
            let cap = match path.linecap {
                LineCap::Butt => "LineCap::Butt",
                LineCap::Round => "LineCap::Round",
                LineCap::Square => "LineCap::Square",
            };
            let join = match path.linejoin {
                LineJoin::Miter => "LineJoin::Miter",
                LineJoin::Round => "LineJoin::Round",
                LineJoin::Bevel => "LineJoin::Bevel",
            };
            let _ = writeln!(w, "{indent}d: {:?},", path.d);
            let _ = writeln!(w, "{indent}fill: {},", path.fill);
            let _ = writeln!(w, "{indent}stroke: {},", path.stroke);
            let _ = writeln!(w, "{indent}linecap: {cap},");
            let _ = writeln!(w, "{indent}linejoin: {join},");
            let _ = writeln!(w, "{indent}miterlimit: {:?},", path.miterlimit);
        };

        if sym.paths.len() == 1 {
            let _ = writeln!(w, "static {ident}_PATHS: &[ArtPath] = &[ArtPath {{");
            write_path_fields(w, &sym.paths[0], "    ");
            let _ = writeln!(w, "}}];");
        } else {
            let _ = writeln!(w, "static {ident}_PATHS: &[ArtPath] = &[");
            for path in &sym.paths {
                let _ = writeln!(w, "    ArtPath {{");
                write_path_fields(w, path, "        ");
                let _ = writeln!(w, "    }},");
            }
            let _ = writeln!(w, "];");
        }
        let _ = writeln!(w);

        let _ = writeln!(w, "static {ident}_ART: SymbolArt = SymbolArt {{");
        let _ = writeln!(w, "    view_box_size: {:?},", VIEW_BOX_UNITS as f32);
        let _ = writeln!(
            w,
            "    original_stroke_units: {:?},",
            sym.original_stroke_units
        );
        let _ = writeln!(w, "    paths: {ident}_PATHS,");
        let _ = writeln!(w, "}};");
        let _ = writeln!(w);

        let _ = writeln!(w, "/// `{}` (`SymbolId({})`).", sym.name, idx);
        let _ = writeln!(w, "#[allow(dead_code)]");
        let _ = writeln!(w, "pub const {ident}: SymbolId = SymbolId({idx});");
        let _ = writeln!(w);
    }

    let _ = writeln!(w, "pub static SYMBOLS: &[SymbolDef] = &[");
    for (idx, sym) in symbols.iter().enumerate() {
        let ident = const_ident(&sym.name);
        let _ = writeln!(w, "    SymbolDef {{");
        let _ = writeln!(w, "        id: SymbolId({idx}),");
        let _ = writeln!(w, "        name: {:?},", sym.name);
        let _ = writeln!(w, "        bounds: Rect {{");
        let _ = writeln!(
            w,
            "            min: {},",
            fmt_pt(sym.bounds.min, "            ")
        );
        let _ = writeln!(
            w,
            "            max: {},",
            fmt_pt(sym.bounds.max, "            ")
        );
        let _ = writeln!(w, "        }},");
        let _ = writeln!(w, "        pins: {ident}_PINS,");
        let _ = writeln!(w, "        art: &{ident}_ART,");
        let _ = writeln!(w, "    }},");
    }
    let _ = writeln!(w, "];");
    out
}

fn const_ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn fmt_i32(v: i32) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let digits = i64::from(v).abs().to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push('_');
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();
    format!("{sign}{grouped}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::workspace_root;

    #[test]
    fn generated_rs_is_up_to_date() {
        let root = workspace_root();
        let expected = render_from_files(&root).expect("render symbols");
        let actual = fs::read_to_string(root.join(GENERATED)).expect("read generated.rs");
        let actual = actual.replace("\r\n", "\n");
        assert!(
            expected == actual,
            "{GENERATED} is stale; run `cargo xtask symbols`"
        );
    }

    #[test]
    fn fails_when_a_pin_is_off_the_lattice() {
        let root = workspace_root();
        let svg_dir = root.join(VENDOR_SVG_DIR);
        // (0, 60) at 80 px maps to an exact integer (-5_080_000, -1_016_000) nm, off 1,270,000 nm.
        let bad_toml = r#"
[symbols.Resistor-IEEE-Standard]
size_px = 80
pins = [
  { number = "1", at = [0.0, 60.0] },
  { number = "2", at = [150.0, 75.0] },
]
"#;
        let specs = parse_pins_toml(bad_toml).expect("parse pins.toml");
        let err = compile_all(&specs, &svg_dir).expect_err("off-lattice pin must fail");
        assert!(
            err.contains("pin lattice"),
            "unexpected error message: {err}"
        );

        let non_int_toml = r#"
[symbols.Resistor-IEEE-Standard]
size_px = 80
pins = [
  { number = "1", at = [0.0, 74.0] },
  { number = "2", at = [150.0, 75.0] },
]
"#;
        let specs = parse_pins_toml(non_int_toml).expect("parse pins.toml");
        let err = compile_all(&specs, &svg_dir).expect_err("fractional-nm pin must fail");
        assert!(
            err.contains("non-integer nm"),
            "unexpected error message: {err}"
        );

        let bad_ground = r#"
[symbols.Ground-COM-General]
size_px = 48
anchor = [75.0, 75.0]
pins = [
  { number = "1", at = [75.0, 0.0] },
]
"#;
        let specs = parse_pins_toml(bad_ground).expect("parse pins.toml");
        let err = compile_all(&specs, &svg_dir)
            .expect_err("center-anchored 48px ground pin is off-lattice");
        assert!(
            err.contains("pin lattice"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn every_vendored_svg_compiles() {
        let root = workspace_root();
        let svg_dir = root.join(VENDOR_SVG_DIR);
        let mut count = 0usize;
        for entry in fs::read_dir(&svg_dir).expect("read vendor SVG dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_some_and(|e| e == "svg") {
                let name = path.file_stem().unwrap().to_str().unwrap();
                let bytes = fs::read(&path).expect("read SVG");
                let (stroke, paths) = extract_svg_paths(name, &bytes, &[])
                    .unwrap_or_else(|e| panic!("failed to compile {name}: {e}"));
                assert!(stroke > 0.0, "{name}: non-positive stroke");
                assert!(!paths.is_empty(), "{name}: empty path list");
                count += 1;
            }
        }
        assert_eq!(count, 116, "expected 116 vendored SVGs");
    }
}
