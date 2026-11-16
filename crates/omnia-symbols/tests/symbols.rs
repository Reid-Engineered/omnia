use std::{fs, path::Path};

use omnia_doc::{GRID_NM, Orient, PIN_LATTICE_NM, Pt};
use omnia_symbols::SymbolLibrary;

const NM_PER_PX: i32 = GRID_NM / 20; // K3: 20 px per 2,540,000 nm grid step = 127,000 nm/px

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

/// Exact port of `pinAt` in `symbols.mjs` (returns placed px coordinates).
fn js_pin_at(x: f64, y: f64, size: f64, orient: Orient, px: f64, py: f64) -> [f64; 2] {
    let k = size / 150.0;
    let mut u = (if orient.is_flipped() { 150.0 - px } else { px }) - 75.0;
    let mut v = py - 75.0;
    for _ in 0..orient.quarter_turns() {
        (u, v) = (-v, u);
    }
    [x + (u + 75.0) * k, y + (v + 75.0) * k]
}

fn rasterise_svg(svg_bytes: &[u8], size_px: u32) -> resvg::tiny_skia::Pixmap {
    let tree = resvg::usvg::Tree::from_data(svg_bytes, &resvg::usvg::Options::default())
        .expect("parse SVG with usvg");
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size_px, size_px).expect("allocate resvg pixmap");
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    let sx = size_px as f32 / tree.size().width();
    let sy = size_px as f32 / tree.size().height();
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap
}

fn diff_fraction(a: &resvg::tiny_skia::Pixmap, b: &resvg::tiny_skia::Pixmap) -> f64 {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let total = (a.width() * a.height()) as f64;
    let mut diff = 0usize;
    for (pa, pb) in a
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0.iter())
    {
        if pa[0].abs_diff(pb[0]) > 2
            || pa[1].abs_diff(pb[1]) > 2
            || pa[2].abs_diff(pb[2]) > 2
            || pa[3].abs_diff(pb[3]) > 2
        {
            diff += 1;
        }
    }
    diff as f64 / total
}

#[test]
fn builtin_library_has_all_six_mockup_symbols() {
    let lib = SymbolLibrary::builtin();
    let expected = [
        "Capacitor-IEEE-NonPolarized",
        "Diode-COM-Standard",
        "Ground-COM-General",
        "IC-COM-OpAmp",
        "Resistor-IEEE-Potentiometer",
        "Resistor-IEEE-Standard",
    ];
    assert_eq!(lib.len(), expected.len());
    assert!(!lib.is_empty());
    assert_eq!(lib.by_name("No-Such-Symbol"), None);

    for (idx, name) in expected.iter().enumerate() {
        let id = lib
            .by_name(name)
            .unwrap_or_else(|| panic!("missing symbol {name}"));
        assert_eq!(id.index(), idx);
        assert_eq!(id.raw(), idx as u16);
        let def = lib.get(id);
        assert_eq!(def.id, id);
        assert_eq!(def.name, *name);
    }

    let iterated: Vec<&str> = lib.iter().map(|d| d.name).collect();
    assert_eq!(iterated, expected);
}

#[test]
fn write_svg_at_original_stroke_matches_vendor_svg_under_resvg() {
    let lib = SymbolLibrary::builtin();
    let svg_dir = workspace_root().join("vendor/electronic-symbols/SVG");

    for def in lib.iter() {
        let vendor_bytes = fs::read(svg_dir.join(format!("{}.svg", def.name)))
            .unwrap_or_else(|e| panic!("read vendor SVG for {}: {e}", def.name));

        let mut generated = Vec::new();
        def.art
            .write_svg(def.art.original_stroke_units, &mut generated);

        // Compare at the 150-unit art box resolution and at placed 100%-zoom size.
        let placed_px = (def.bounds.width() / i64::from(NM_PER_PX)) as u32;
        for raster_px in [150_u32, placed_px] {
            let vendor_pm = rasterise_svg(&vendor_bytes, raster_px);
            let gen_pm = rasterise_svg(&generated, raster_px);
            let frac = diff_fraction(&vendor_pm, &gen_pm);
            assert!(
                frac < 0.01,
                "{} at {raster_px}x{raster_px}: pixel diff {frac:.4} >= 0.01",
                def.name
            );
        }

        // Also verify widened strokes for far-zoom 1-device-px floor parse and render cleanly.
        let mut widened = Vec::new();
        def.art.write_svg(12.0, &mut widened);
        let widened_pm = rasterise_svg(&widened, 32);
        assert!(
            widened_pm
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] < 250),
            "{}: widened SVG rendered blank",
            def.name
        );
    }
}

#[test]
fn pins_land_on_k2_lattice_and_match_symbols_mjs() {
    let lib = SymbolLibrary::builtin();

    for def in lib.iter() {
        assert!(!def.pins.is_empty(), "{}: has no pins", def.name);
        for pin in def.pins {
            assert_eq!(
                pin.at.x % PIN_LATTICE_NM,
                0,
                "{} pin {} x={} off 1.27 mm lattice",
                def.name,
                pin.number,
                pin.at.x
            );
            assert_eq!(
                pin.at.y % PIN_LATTICE_NM,
                0,
                "{} pin {} y={} off 1.27 mm lattice",
                def.name,
                pin.number,
                pin.at.y
            );
            assert!(
                def.bounds.contains(pin.at),
                "{} pin {} ({:?}) outside bounds {:?}",
                def.name,
                pin.number,
                pin.at,
                def.bounds
            );
        }
    }

    // Check K7 placed sizes and exact `pinAt` correspondence for box-centered symbols.
    struct CenteredCase {
        name: &'static str,
        size_px: f64,
        pins: &'static [(&'static str, f64, f64)],
    }
    let centered_cases = [
        CenteredCase {
            name: "Resistor-IEEE-Standard",
            size_px: 80.0,
            pins: &[("1", 0.0, 75.0), ("2", 150.0, 75.0)],
        },
        CenteredCase {
            name: "Resistor-IEEE-Potentiometer",
            size_px: 80.0,
            pins: &[("1", 0.0, 75.0), ("2", 150.0, 75.0), ("3", 75.0, 150.0)],
        },
        CenteredCase {
            name: "Capacitor-IEEE-NonPolarized",
            size_px: 80.0,
            pins: &[("1", 0.0, 75.0), ("2", 150.0, 75.0)],
        },
        CenteredCase {
            name: "Diode-COM-Standard",
            size_px: 80.0,
            pins: &[("1", 0.0, 75.0), ("2", 150.0, 75.0)],
        },
        CenteredCase {
            name: "IC-COM-OpAmp",
            size_px: 180.0,
            pins: &[
                ("3", 0.0, 50.0),
                ("2", 0.0, 100.0),
                ("1", 150.0, 75.0),
                ("8", 75.0, 0.0),
                ("4", 75.0, 150.0),
            ],
        },
    ];

    let box_x = 1000.0;
    let box_y = 300.0;
    for case in centered_cases {
        let CenteredCase {
            name,
            size_px,
            pins: expected_pins,
        } = case;
        let def = lib.get(lib.by_name(name).unwrap());
        assert_eq!(def.bounds.width(), (size_px as i64) * i64::from(NM_PER_PX));
        assert_eq!(def.bounds.height(), (size_px as i64) * i64::from(NM_PER_PX));

        let center = Pt::new(
            ((box_x + size_px / 2.0) as i32) * NM_PER_PX,
            ((box_y + size_px / 2.0) as i32) * NM_PER_PX,
        )
        .unwrap();

        assert_eq!(def.pins.len(), expected_pins.len());
        for (pin, &(num, svg_x, svg_y)) in def.pins.iter().zip(expected_pins) {
            assert_eq!(pin.number, num);
            for orient in Orient::ALL {
                let world_pin = center + orient.apply(pin.at);
                let [js_x, js_y] = js_pin_at(box_x, box_y, size_px, orient, svg_x, svg_y);
                assert_eq!(world_pin.x, (js_x.round() as i32) * NM_PER_PX);
                assert_eq!(world_pin.y, (js_y.round() as i32) * NM_PER_PX);
            }
        }
    }

    // Ground is 48 px anchored by its pin (K7) so the pin stays on the K2 lattice under any Orient.
    let gnd = lib.get(lib.by_name("Ground-COM-General").unwrap());
    assert_eq!(gnd.bounds.width(), 48 * i64::from(NM_PER_PX));
    assert_eq!(gnd.bounds.height(), 48 * i64::from(NM_PER_PX));
    assert_eq!(gnd.pins.len(), 1);
    assert_eq!(gnd.pins[0].at, Pt::ZERO);
    // In build-omnia.mjs: G3 = { x: 976, y: 600, size: 48 }, pinAt(G3, 75, 0) = [1000, 600].
    let [js_gx, js_gy] = js_pin_at(976.0, 600.0, 48.0, Orient::IDENTITY, 75.0, 0.0);
    let g3_at = Pt::new(1000 * NM_PER_PX, 600 * NM_PER_PX).unwrap();
    assert_eq!(g3_at.x, (js_gx.round() as i32) * NM_PER_PX);
    assert_eq!(g3_at.y, (js_gy.round() as i32) * NM_PER_PX);
    assert_eq!(
        g3_at + gnd.bounds.min,
        Pt::new(976 * NM_PER_PX, 600 * NM_PER_PX).unwrap()
    );
    assert_eq!(
        g3_at + gnd.bounds.max,
        Pt::new(1024 * NM_PER_PX, 648 * NM_PER_PX).unwrap()
    );

    // Check Wien oscillator R3 (rot 90 at { x: 960, y: 520, size: 80 }) meets G3 at (1000, 600).
    let pot = lib.get(lib.by_name("Resistor-IEEE-Potentiometer").unwrap());
    let r3_at = Pt::new(1000 * NM_PER_PX, 560 * NM_PER_PX).unwrap();
    assert_eq!(
        r3_at + Orient::ROT_90.apply(pot.pins[0].at),
        Pt::new(1000 * NM_PER_PX, 520 * NM_PER_PX).unwrap()
    );
    assert_eq!(
        r3_at + Orient::ROT_90.apply(pot.pins[1].at),
        g3_at + Orient::IDENTITY.apply(gnd.pins[0].at)
    );
    assert_eq!(
        r3_at + Orient::ROT_90.apply(pot.pins[2].at),
        Pt::new(960 * NM_PER_PX, 560 * NM_PER_PX).unwrap()
    );
}
