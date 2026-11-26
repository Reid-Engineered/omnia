//! D4/G4/K11: one alpha-mask sprite per symbol, with placement in a device-space matrix.

use crate::{camera::Camera, snap_px::device_rect};
use gpui::{Bounds, Pixels, SharedString, TransformationMatrix};
use omnia_doc::{Orient, Part, Pt, SymbolId};
use omnia_symbols::{SymbolDef, SymbolLibrary};

pub(super) const SYMBOL_STROKE_NM: f64 = 254_000.0;
// Bound the atlas raster at high zoom. The instance matrix maps it to its true size.
const MAX_BUCKET: u32 = 40; // 2^(40/4) = 1024 device pixels (2048 mask texels).
const DOT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><circle cx="4" cy="4" r="4"/></svg>"#;

#[derive(Debug)]
pub(crate) struct Mask {
    pub(crate) key: SharedString,
    pub(crate) svg: Vec<u8>,
    side: u32,
}

#[derive(Debug)]
pub(crate) struct Masks {
    library: std::sync::Arc<SymbolLibrary>,
    symbols: Vec<Mask>,
    dot: Mask,
}

impl Masks {
    pub(crate) fn new() -> Self {
        let library = SymbolLibrary::builtin();
        let symbols = library
            .iter()
            .flat_map(|def| {
                (0..=MAX_BUCKET).map(move |bucket| {
                    let side = (2f64.powf(f64::from(bucket) / 4.0).floor() as u32).max(1);
                    let art = def.art;
                    let stroke = (SYMBOL_STROKE_NM / world_side(def)).max(1.0 / f64::from(side));
                    let mut svg = Vec::new();
                    art.write_svg((stroke * f64::from(art.view_box_size)) as f32, &mut svg);
                    Mask {
                        key: format!("omnia-sheet-symbol-{}-{bucket}", def.id.raw()).into(),
                        svg,
                        side,
                    }
                })
            })
            .collect();
        Self {
            library,
            symbols,
            dot: Mask {
                key: "omnia-sheet-junction".into(),
                svg: DOT.to_vec(),
                side: 16,
            },
        }
    }

    pub(crate) fn symbol_def(&self, symbol: SymbolId) -> Option<&SymbolDef> {
        (symbol.index() < self.library.len()).then(|| self.library.get(symbol))
    }

    pub(crate) fn symbol_at(
        &self,
        symbol: SymbolId,
        at: Pt,
        orient: Orient,
        camera: &Camera,
        sf: f32,
    ) -> Option<Shape<'_>> {
        let def = self.symbol_def(symbol)?;
        let side = world_side(def) * camera.scale();
        let mid = Pt {
            x: def.bounds.min.x.midpoint(def.bounds.max.x),
            y: def.bounds.min.y.midpoint(def.bounds.max.y),
        };
        let offset = orient.apply(mid);
        let anchor = camera.world_to_screen(at);
        let x = (anchor.x + f64::from(offset.x) * camera.scale()) * f64::from(sf);
        let y = (anchor.y + f64::from(offset.y) * camera.scale()) * f64::from(sf);
        let device_side = side * f64::from(sf);
        let bucket = ((device_side.log2() * 4.0).floor() as u32).min(MAX_BUCKET);
        let mask = &self.symbols[symbol.index() * (MAX_BUCKET + 1) as usize + bucket as usize];
        Some(Shape {
            x,
            y,
            logical_side: side,
            orient,
            mask,
        })
    }

    pub(super) fn symbol(&self, part: &Part, camera: &Camera, sf: f32) -> Option<Shape<'_>> {
        self.symbol_at(part.symbol, part.at, part.orient, camera, sf)
    }

    pub(super) fn dot(&self, x: f64, y: f64, diameter: f64, sf: f32) -> Shape<'_> {
        Shape {
            x: x * f64::from(sf),
            y: y * f64::from(sf),
            logical_side: diameter,
            orient: Orient::IDENTITY,
            mask: &self.dot,
        }
    }
}

fn world_side(def: &SymbolDef) -> f64 {
    f64::from(def.bounds.max.x - def.bounds.min.x)
}

pub(crate) struct Shape<'a> {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) logical_side: f64,
    orient: Orient,
    pub(crate) mask: &'a Mask,
}

impl Shape<'_> {
    pub(crate) fn sprite(&self, sf: f32) -> (Bounds<Pixels>, TransformationMatrix) {
        let raster = self.mask.side;
        let half = f64::from(raster) / 2.0;
        let left = (self.x - half).round();
        let top = (self.y - half).round();
        let bounds = device_rect(left, top, raster, raster, sf);
        let ratio = (self.logical_side * f64::from(sf) / f64::from(raster)) as f32;
        let u = self.orient.apply(Pt { x: 1, y: 0 });
        let v = self.orient.apply(Pt { x: 0, y: 1 });
        let [[a, b], [c, d]] = [
            [u.x as f32 * ratio, v.x as f32 * ratio],
            [u.y as f32 * ratio, v.y as f32 * ratio],
        ];
        let (cx, cy) = ((left + half) as f32, (top + half) as f32);
        (
            bounds,
            TransformationMatrix {
                rotation_scale: [[a, b], [c, d]],
                translation: [
                    self.x as f32 - a * cx - b * cy,
                    self.y as f32 - c * cx - d * cy,
                ],
            },
        )
    }
}
