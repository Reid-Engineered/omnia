//! Scripted camera driver. The camera moves one device pixel per frame, so every frame
//! repaints with fresh pixel snapping and no frame is skipped as identical.
//!
//! T2.6 adds `zoom`, `drag-part`, `drag-wire` and `hover` over real sheets in `scripts.rs`.

/// Camera offset from the sheet origin, in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
}

impl Camera {
    /// The offset in logical pixels for a window at `scale_factor`.
    pub fn logical(self, scale_factor: f32) -> (f32, f32) {
        (self.x / scale_factor, self.y / scale_factor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// Pans right by one device pixel per frame.
    Pan,
}

impl Script {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "pan" => Ok(Self::Pan),
            "zoom" | "drag-part" | "drag-wire" | "hover" => Err(format!(
                "script `{name}` needs real sheets; roadmap T2.6 adds it"
            )),
            _ => Err(format!("unknown script `{name}` (available: pan)")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pan => "pan",
        }
    }

    /// The camera for frame `frame`, counting from the first rendered frame.
    pub fn camera(self, frame: u32) -> Camera {
        match self {
            Self::Pan => Camera {
                x: frame as f32,
                y: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_moves_one_device_pixel_per_frame() {
        for frame in [0_u32, 1, 2, 599, 10_000] {
            let step = Script::Pan.camera(frame + 1).x - Script::Pan.camera(frame).x;
            assert_eq!(step, 1.0);
            assert_eq!(Script::Pan.camera(frame).y, 0.0);
        }
        let (x, _) = Script::Pan.camera(3).logical(1.5);
        assert_eq!(x, 2.0);
    }

    #[test]
    fn parse_scripts() {
        assert_eq!(Script::parse("pan"), Ok(Script::Pan));
        assert!(Script::parse("zoom").unwrap_err().contains("T2.6"));
        assert!(Script::parse("spin").is_err());
    }
}
