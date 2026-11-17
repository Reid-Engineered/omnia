//! Vector icon element backed by embedded Tabler outline assets and theme tokens (UD17, UINV1).

use crate::{assets, theme};
use gpui::{App, IntoElement, Pixels, RenderOnce, SharedString, Window, prelude::*, px, svg};
use omnia_theme::Rgba;

/// Default logical icon size (toolbar icon size, UK3).
pub const DEFAULT_ICON_SIZE: f32 = 18.0;

/// Renders a vendored Tabler outline icon at a given size tinted with a theme token colour.
#[derive(Clone, Debug, IntoElement)]
pub struct Icon {
    name: SharedString,
    size: Pixels,
    color: Option<Rgba>,
}

impl Icon {
    /// Creates an icon for `name` (Tabler outline name without `.svg`, e.g. `"file-plus"`).
    ///
    /// Defaults to [`DEFAULT_ICON_SIZE`] (18 px) and the active theme's `text_secondary` token.
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            size: px(DEFAULT_ICON_SIZE),
            color: None,
        }
    }

    /// Sets the logical width and height of the icon.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Sets the token colour used to tint the icon (via [`theme::color`]).
    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn icon_size(&self) -> Pixels {
        self.size
    }

    pub fn icon_color(&self) -> Option<Rgba> {
        self.color
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self
            .color
            .unwrap_or_else(|| theme::theme(cx).text_secondary);
        svg()
            .path(assets::icon_asset_path(&self.name))
            .size(self.size)
            .flex_none()
            .text_color(theme::color(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Assets, ICON_NAMES};
    use crate::theme::ActiveTheme;
    use gpui::{
        AppContext, AssetSource, Context, HeadlessAppContext, NoopTextSystem, Render, div, size,
    };
    use omnia_theme::{GRAPHITE, GRAY};
    use std::{
        borrow::Cow,
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct TrackingAssets {
        requested: Mutex<Vec<String>>,
        missing: Mutex<Vec<String>>,
        errors: Mutex<Vec<String>>,
    }

    impl AssetSource for TrackingAssets {
        fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
            self.requested.lock().unwrap().push(path.to_owned());
            match Assets.load(path) {
                Ok(Some(bytes)) => Ok(Some(bytes)),
                Ok(None) => {
                    if path.starts_with("icons/") {
                        self.missing.lock().unwrap().push(path.to_owned());
                    }
                    Ok(None)
                }
                Err(err) => {
                    self.errors.lock().unwrap().push(format!("{path}: {err}"));
                    Err(err)
                }
            }
        }

        fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
            Assets.list(path)
        }
    }

    struct AllIconsGallery {
        names: Vec<&'static str>,
    }

    impl Render for AllIconsGallery {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let active = theme::theme(cx);
            div()
                .size_full()
                .flex()
                .flex_wrap()
                .bg(theme::color(active.bg_panel))
                .children(self.names.iter().enumerate().map(|(i, &name)| match i % 3 {
                    0 => Icon::new(name).size(px(16.0)).color(active.text_secondary),
                    1 => Icon::new(name).size(px(18.0)).color(active.text_primary),
                    _ => Icon::new(name).size(px(20.0)).color(active.accent_mint),
                }))
        }
    }

    #[test]
    fn icon_renders_every_vendored_asset_headlessly_without_missing_assets() {
        let tracker = Arc::new(TrackingAssets::default());
        let mut cx =
            HeadlessAppContext::with_asset_source(Arc::new(NoopTextSystem::new()), tracker.clone());
        cx.update(ActiveTheme::install);

        let window = cx
            .open_window(size(px(640.0), px(480.0)), |_, cx| {
                cx.new(|_| AllIconsGallery {
                    names: ICON_NAMES.to_vec(),
                })
            })
            .expect("open headless window");

        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
        })
        .expect("draw all icons headlessly");

        let missing = tracker.missing.lock().unwrap();
        assert!(missing.is_empty(), "missing icon assets: {missing:?}");
        let errors = tracker.errors.lock().unwrap();
        assert!(errors.is_empty(), "icon asset load errors: {errors:?}");

        let requested = tracker.requested.lock().unwrap();
        for &name in ICON_NAMES {
            let expected = assets::icon_asset_path(name);
            assert!(
                requested.iter().any(|p| p == expected.as_ref()),
                "icon {name} ({expected}) was never requested during draw"
            );
        }
    }

    #[test]
    fn icon_builder_defaults_and_overrides_work() {
        let default_icon = Icon::new("file-plus");
        assert_eq!(default_icon.name(), "file-plus");
        assert_eq!(default_icon.icon_size(), px(DEFAULT_ICON_SIZE));
        assert_eq!(default_icon.icon_color(), None);

        let custom_icon = Icon::new("settings")
            .size(px(20.0))
            .color(GRAY.text_primary);
        assert_eq!(custom_icon.name(), "settings");
        assert_eq!(custom_icon.icon_size(), px(20.0));
        assert_eq!(custom_icon.icon_color(), Some(GRAY.text_primary));
        assert_ne!(
            theme::color(GRAPHITE.text_primary),
            theme::color(GRAY.text_primary)
        );
    }
}
