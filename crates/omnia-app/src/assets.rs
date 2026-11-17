//! Embedded Tabler icon assets served through GPUI's [`AssetSource`] (UD17, UG14).

use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

struct IconAsset {
    name: &'static str,
    path: &'static str,
    bytes: &'static [u8],
}

macro_rules! tabler_icons {
    ($($name:literal),* $(,)?) => {
        const ICON_ASSETS: &[IconAsset] = &[
            $(
                IconAsset {
                    name: $name,
                    path: concat!("icons/", $name, ".svg"),
                    bytes: include_bytes!(concat!("../../../vendor/tabler-icons/outline/", $name, ".svg")),
                },
            )*
        ];

        /// Every vendored Tabler outline icon name, sorted alphabetically.
        pub const ICON_NAMES: &[&str] = &[$($name),*];
    };
}

tabler_icons!(
    "alert-triangle",
    "arrow-back-up",
    "arrow-forward-up",
    "arrows-exchange",
    "arrows-maximize",
    "binary",
    "bolt",
    "books",
    "chart-dots",
    "chart-histogram",
    "chart-line",
    "chart-ppf",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "clipboard",
    "clock",
    "components",
    "copy",
    "cpu",
    "cpu-2",
    "device-desktop-analytics",
    "device-floppy",
    "file",
    "file-plus",
    "folder",
    "folder-open",
    "gauge",
    "home",
    "info-circle",
    "keyboard",
    "layout-bottombar",
    "layout-grid",
    "layout-sidebar",
    "maximize",
    "minus",
    "needle",
    "network",
    "player-pause",
    "player-play",
    "player-stop",
    "plus",
    "printer",
    "restore",
    "route",
    "scissors",
    "search",
    "settings",
    "square",
    "stopwatch",
    "table",
    "trash",
    "user",
    "wave-saw-tool",
    "wave-sine",
    "wave-square",
    "x",
    "zoom-in",
    "zoom-in-area",
    "zoom-out",
);

fn find_icon(name: &str) -> Option<&'static IconAsset> {
    ICON_ASSETS
        .binary_search_by_key(&name, |entry| entry.name)
        .ok()
        .map(|idx| &ICON_ASSETS[idx])
}

/// Returns the embedded SVG bytes for a Tabler icon name, if present.
pub fn icon_bytes(name: &str) -> Option<&'static [u8]> {
    find_icon(name).map(|entry| entry.bytes)
}

/// Returns the asset-source path for a Tabler icon name (`"icons/<name>.svg"`).
///
/// Known icon names return a static `SharedString` with no heap allocation.
pub fn icon_asset_path(name: &str) -> SharedString {
    if let Some(entry) = find_icon(name) {
        SharedString::new_static(entry.path)
    } else {
        SharedString::from(format!("icons/{name}.svg"))
    }
}

/// Application asset source serving the embedded Tabler outline icons (UD17, UG14).
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path
            .strip_prefix("icons/")
            .and_then(|rest| rest.strip_suffix(".svg"))
        else {
            return Ok(None);
        };
        Ok(icon_bytes(name).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let prefix = path.trim_end_matches('/');
        if prefix.is_empty() || prefix == "icons" {
            Ok(ICON_ASSETS
                .iter()
                .map(|entry| SharedString::new_static(entry.path))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AssetSource, DevicePixels, Size, SvgRenderer, SvgSize};
    use std::{fs, path::Path, sync::Arc};

    /// Every Tabler icon used in `design/build-omnia.mjs` (48) plus the UD17 shell additions (13).
    const REQUIRED_ICONS: &[&str] = &[
        // Mockup (48)
        "arrow-back-up",
        "arrow-forward-up",
        "arrows-exchange",
        "arrows-maximize",
        "binary",
        "bolt",
        "books",
        "chart-dots",
        "chart-histogram",
        "chart-line",
        "chart-ppf",
        "chevron-down",
        "clipboard",
        "clock",
        "components",
        "copy",
        "cpu",
        "cpu-2",
        "device-desktop-analytics",
        "device-floppy",
        "file-plus",
        "folder",
        "folder-open",
        "gauge",
        "home",
        "layout-grid",
        "maximize",
        "minus",
        "needle",
        "network",
        "player-pause",
        "player-play",
        "player-stop",
        "plus",
        "printer",
        "route",
        "scissors",
        "search",
        "square",
        "stopwatch",
        "user",
        "wave-saw-tool",
        "wave-sine",
        "wave-square",
        "x",
        "zoom-in",
        "zoom-in-area",
        "zoom-out",
        // UD17 additions (13)
        "alert-triangle",
        "check",
        "chevron-left",
        "chevron-right",
        "file",
        "info-circle",
        "keyboard",
        "layout-bottombar",
        "layout-sidebar",
        "restore",
        "settings",
        "table",
        "trash",
    ];

    #[test]
    fn icon_names_are_sorted_and_match_vendored_files() {
        assert!(
            ICON_NAMES.windows(2).all(|w| w[0] < w[1]),
            "ICON_NAMES must stay strictly sorted for binary search"
        );
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/tabler-icons/outline");
        let mut on_disk: Vec<String> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(|entry| {
                let path = entry.expect("dir entry").path();
                (path.extension().and_then(|s| s.to_str()) == Some("svg"))
                    .then(|| path.file_stem().unwrap().to_str().unwrap().to_owned())
            })
            .collect();
        on_disk.sort();
        let embedded: Vec<String> = ICON_NAMES.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(embedded, on_disk);

        for &name in REQUIRED_ICONS {
            assert!(
                icon_bytes(name).is_some(),
                "required icon {name:?} missing from Assets"
            );
        }
    }

    #[test]
    fn asset_source_loads_lists_and_returns_none_for_missing() {
        let assets = Assets;
        for &name in ICON_NAMES {
            let path = icon_asset_path(name);
            let bytes = assets
                .load(&path)
                .unwrap_or_else(|e| panic!("load {path}: {e}"))
                .unwrap_or_else(|| panic!("missing embedded icon {path}"));
            let text = std::str::from_utf8(&bytes).expect("SVG bytes must be UTF-8");
            assert!(text.contains("<svg"), "{path} is not an SVG");
        }
        assert_eq!(assets.load("icons/does-not-exist.svg").unwrap(), None);
        assert_eq!(assets.load("other/file-plus.svg").unwrap(), None);

        let listed_root = assets.list("").unwrap();
        let listed_icons = assets.list("icons").unwrap();
        assert_eq!(listed_root.len(), ICON_NAMES.len());
        assert_eq!(listed_icons, listed_root);
        assert!(assets.list("other").unwrap().is_empty());
    }

    #[test]
    fn svg_renderer_rasterizes_every_icon_at_shell_sizes() {
        let renderer = SvgRenderer::new(Arc::new(Assets));
        for &name in ICON_NAMES {
            let path = icon_asset_path(name);
            let bytes = Assets
                .load(&path)
                .unwrap_or_else(|e| panic!("load {path}: {e}"))
                .unwrap_or_else(|| panic!("missing embedded icon {path}"));
            let parsed = renderer
                .parse_svg(&bytes)
                .unwrap_or_else(|e| panic!("parse {name}: {e}"));
            for px_size in [16, 18, 20] {
                let target = Size::new(DevicePixels(px_size * 2), DevicePixels(px_size * 2));
                let image = renderer
                    .render_parsed(&parsed, SvgSize::ExactSize(target))
                    .unwrap_or_else(|e| panic!("render {name} at {px_size}px: {e}"));
                assert_eq!(image.size(0), target);
                let raw = image.as_bytes(0).expect("frame 0 bytes");
                assert!(
                    raw.as_chunks::<4>().0.iter().any(|bgra| bgra[3] > 0),
                    "{name} at {px_size}px rasterized to an empty image"
                );
            }
        }
    }
}
