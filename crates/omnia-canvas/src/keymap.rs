//! T5.2: canvas actions and their default key bindings (UD3, UQ14, canvas-plan section 5).
//!
//! Every canvas command is a GPUI action declared here once; the shell re-uses these types
//! and never redeclares them (UD3). Actions resolve in the key context [`SHEET_CONTEXT`],
//! which the canvas views set on the element that tracks their focus, so a key reaches them
//! only while the sheet holds GPUI focus. Their handlers are registered by
//! `tool::sheet_key_context`.
//!
//! Two binding sets:
//!
//! - [`sheet_bindings`]: the single-key tools R, X, Y and W, bound in `Sheet` only, so typing
//!   in a text field never rotates a part (UD3, UINV4). The shell appends these to its own
//!   keymap (U5.1).
//! - [`standalone_bindings`]: the same four plus Esc, Del, Ctrl+Z and Ctrl+Y, also in `Sheet`,
//!   for a canvas hosted without the shell (tests, bench and canvas harnesses). The shell
//!   never registers these four: it binds Del, Ctrl+Z and Ctrl+Y in `Workspace` to its own
//!   actions and Esc in `Shell` (`NavCancel`), and its handlers call `SheetCanvas::undo`,
//!   `redo`, `cancel` and (T5.3) `delete_selection` (UD3, UQ14). Registering both sets in
//!   one window would give Del, Ctrl+Z and Ctrl+Y two bindings on one context stack.

use gpui::KeyBinding;

/// The key context of the focused sheet (UD3 `KeyContext::Sheet`).
pub const SHEET_CONTEXT: &str = "Sheet";

gpui::actions!(
    canvas,
    [
        /// R: rotate a quarter turn (`Orient::ROT_90`).
        Rotate,
        /// X: mirror left to right (`Orient::FLIP_X`).
        MirrorHorizontal,
        /// Y: mirror top to bottom (`Orient::FLIP_Y`).
        MirrorVertical,
        /// W: start the wire tool.
        PlaceWire,
        /// Esc without the shell: cancel the gesture or tool (`SheetCanvas::cancel`).
        Cancel,
        /// Del without the shell: delete the selection (T5.3).
        Delete,
        /// Ctrl+Z without the shell: undo one document step.
        Undo,
        /// Ctrl+Y without the shell: redo one document step.
        Redo,
    ]
);

/// Unmodified single-key tools, bound in `Sheet` (UD3). The shell registers these.
pub fn sheet_bindings() -> Vec<KeyBinding> {
    let cx = Some(SHEET_CONTEXT);
    vec![
        KeyBinding::new("r", Rotate, cx),
        KeyBinding::new("x", MirrorHorizontal, cx),
        KeyBinding::new("y", MirrorVertical, cx),
        KeyBinding::new("w", PlaceWire, cx),
    ]
}

/// [`sheet_bindings`] plus Esc, Del, Ctrl+Z and Ctrl+Y in `Sheet`, for a canvas without the
/// shell. Never registered together with the shell's keymap (see the module docs).
pub fn standalone_bindings() -> Vec<KeyBinding> {
    let cx = Some(SHEET_CONTEXT);
    let mut bindings = sheet_bindings();
    bindings.extend([
        KeyBinding::new("escape", Cancel, cx),
        KeyBinding::new("delete", Delete, cx),
        KeyBinding::new("ctrl-z", Undo, cx),
        KeyBinding::new("ctrl-y", Redo, cx),
    ]);
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Action, KeyContext, OperatingSystem};

    fn sheet_stack() -> Vec<KeyContext> {
        vec![KeyContext::parse(SHEET_CONTEXT).unwrap()]
    }

    fn other_stack() -> Vec<KeyContext> {
        vec![KeyContext::parse("TextField").unwrap()]
    }

    fn table(bindings: &[KeyBinding]) -> Vec<(String, &'static str)> {
        bindings
            .iter()
            .map(|b| {
                let keys = b
                    .keystrokes()
                    .iter()
                    .map(|k| k.inner().unparse(OperatingSystem::Windows))
                    .collect::<Vec<_>>()
                    .join(" ");
                (keys, b.action().name())
            })
            .collect()
    }

    #[test]
    fn t5_2_sheet_bindings_are_r_x_y_w() {
        assert_eq!(
            table(&sheet_bindings()),
            vec![
                ("r".to_string(), Rotate.name()),
                ("x".to_string(), MirrorHorizontal.name()),
                ("y".to_string(), MirrorVertical.name()),
                ("w".to_string(), PlaceWire.name()),
            ]
        );
    }

    #[test]
    fn t5_2_standalone_bindings_add_esc_del_undo_redo() {
        assert_eq!(
            table(&standalone_bindings()),
            vec![
                ("r".to_string(), Rotate.name()),
                ("x".to_string(), MirrorHorizontal.name()),
                ("y".to_string(), MirrorVertical.name()),
                ("w".to_string(), PlaceWire.name()),
                ("escape".to_string(), Cancel.name()),
                ("delete".to_string(), Delete.name()),
                ("ctrl-z".to_string(), Undo.name()),
                ("ctrl-y".to_string(), Redo.name()),
            ]
        );
    }

    #[test]
    fn t5_2_every_binding_resolves_only_in_sheet() {
        // UD3: R, X, Y and W live in `Sheet`, so a focused text field never sees them.
        for binding in standalone_bindings() {
            let predicate = binding
                .predicate()
                .expect("every canvas binding has a context");
            assert!(predicate.depth_of(&sheet_stack()).is_some());
            assert!(predicate.depth_of(&other_stack()).is_none());
        }
    }

    #[test]
    fn t5_2_actions_live_in_the_canvas_namespace() {
        for binding in standalone_bindings() {
            assert!(binding.action().name().starts_with("canvas::"));
        }
    }
}
