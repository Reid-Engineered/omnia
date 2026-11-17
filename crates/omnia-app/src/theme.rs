//! Theme plumbing for the shell (UD13, UINV1, UQ6).
//!
//! `ActiveTheme` is the app global every shell view reads at render; `color` is the
//! only token-to-GPUI conversion in `omnia-app`. Theme switching itself is U3.2.

use gpui::{App, Global, Hsla};
use omnia_theme::{GRAPHITE, GRAY, Rgba, Theme};

/// The user's theme preference (UD13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    Graphite,
    Gray,
    System,
}

impl ThemeChoice {
    /// The theme for this choice without a window. `System` falls back to Graphite
    /// until U3.2 follows the window appearance.
    pub const fn resolve(self) -> &'static Theme {
        match self {
            Self::Graphite | Self::System => &GRAPHITE,
            Self::Gray => &GRAY,
        }
    }
}

/// The active theme global (UD13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveTheme {
    pub theme: &'static Theme,
    pub choice: ThemeChoice,
}

impl ActiveTheme {
    pub const fn new(choice: ThemeChoice) -> Self {
        Self {
            theme: choice.resolve(),
            choice,
        }
    }

    /// Installs the default global when none is present yet.
    pub fn install(cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
    }
}

impl Default for ActiveTheme {
    fn default() -> Self {
        Self::new(ThemeChoice::default())
    }
}

impl Global for ActiveTheme {}

/// The active theme, or Graphite when the global has not been installed.
pub fn theme(cx: &App) -> &'static Theme {
    cx.try_global::<ActiveTheme>()
        .map_or(&GRAPHITE, |active| active.theme)
}

/// The only token-to-GPUI conversion in `omnia-app` (UD13).
pub fn color(c: Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    const CALLS: [&str; 3] = ["rgb", "rgba", "hsla"];

    fn is_word(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_'
    }

    /// Every colour literal in `text`, as `(line, excerpt)`. Mirrors INV6 in
    /// `omnia-theme/tests/inv6.rs`, extended to the shell by UINV1.
    fn colour_literals(text: &str) -> Vec<(usize, String)> {
        let bytes = text.as_bytes();
        let mut found = Vec::new();
        let line_of = |i: usize| text[..i].bytes().filter(|&b| b == b'\n').count() + 1;
        for i in 0..bytes.len() {
            let after_word = i > 0 && is_word(bytes[i - 1]);
            if bytes[i] == b'#' && !after_word {
                let digits = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_hexdigit())
                    .count();
                let end = i + 1 + digits;
                let whole = end == bytes.len() || !is_word(bytes[end]);
                if whole && matches!(digits, 3 | 4 | 6 | 8) {
                    found.push((line_of(i), text[i..end].to_owned()));
                }
            }
            if !after_word {
                for call in CALLS {
                    let Some(rest) = bytes[i..].strip_prefix(call.as_bytes()) else {
                        continue;
                    };
                    if rest.first().is_some_and(|&b| is_word(b)) {
                        continue;
                    }
                    if rest.trim_ascii_start().starts_with(b"(") {
                        found.push((line_of(i), format!("{call}(")));
                    }
                }
            }
        }
        found
    }

    fn scan(dir: &Path, hits: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                scan(&path, hits);
                continue;
            }
            let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let text = String::from_utf8_lossy(&bytes);
            for (line, literal) in colour_literals(&text) {
                hits.push(format!("{}:{line}: {literal}", path.display()));
            }
        }
    }

    #[test]
    fn uinv1_omnia_app_has_no_colour_literal() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert!(src.is_dir(), "{} is missing", src.display());
        let mut hits = Vec::new();
        scan(&src, &mut hits);
        assert!(
            hits.is_empty(),
            "UINV1: colour literals in omnia-app; take colours from omnia-theme via theme::color instead:\n{}",
            hits.join("\n")
        );
    }

    #[test]
    fn uinv1_detector_finds_planted_literals() {
        // Built without writing a literal in this scanned file.
        let hex = "#".to_owned() + "2F6FEB";
        let short = "#".to_owned() + "fff";
        let alpha = "#".to_owned() + "00000022";
        let call = "rgb".to_owned() + "(1)";
        let spaced = "hsla".to_owned() + " (0.5, 0.5, 0.5, 1.0)";
        for planted in [hex, short, alpha, call, spaced] {
            assert_eq!(colour_literals(&planted).len(), 1, "missed: {planted}");
        }
    }

    #[test]
    fn uinv1_detector_ignores_lookalikes() {
        let clean = [
            "#[derive(Clone)]",
            "fn to_rgb() {}",
            "let v = rgba_from(theme);",
            "use omnia_theme::Rgba;",
        ];
        for line in clean {
            assert!(colour_literals(line).is_empty(), "false positive: {line}");
        }
    }

    #[test]
    fn active_theme_defaults_to_graphite() {
        let active = ActiveTheme::default();
        assert_eq!(active.choice, ThemeChoice::Graphite);
        assert!(std::ptr::eq(active.theme, &GRAPHITE));
        assert!(std::ptr::eq(ThemeChoice::Gray.resolve(), &GRAY));
        assert!(std::ptr::eq(ThemeChoice::System.resolve(), &GRAPHITE));
    }

    #[test]
    fn color_preserves_alpha_and_matches_gpui_conversion() {
        let token = Rgba::new(0.25, 0.5, 0.75, 0.5);
        let expected: Hsla = gpui::Rgba {
            r: token.r,
            g: token.g,
            b: token.b,
            a: token.a,
        }
        .into();
        assert_eq!(color(token), expected);
        assert_eq!(color(token).a, 0.5);
    }

    #[test]
    fn uq6_overlay_tokens_have_default_values() {
        for theme in [&GRAPHITE, &GRAY] {
            assert_eq!(
                theme.brand_tile,
                Rgba::new(0.9490196, 0.9490196, 0.9490196, 1.0)
            );
            assert_eq!(
                theme.brand_mark,
                Rgba::new(0.06666667, 0.06666667, 0.07058824, 1.0)
            );
            assert_eq!(
                theme.instrument_screen,
                Rgba::new(0.05490196, 0.05882353, 0.06666667, 1.0)
            );
            assert_eq!(theme.focus_ring, theme.stage_select);
        }
    }
}
