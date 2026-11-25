//! T5.2: canvas actions and their standalone key bindings (UD3, UQ14, canvas-plan section 5).
//!
//! Every canvas command is a GPUI action declared here once. The shell re-uses these types in
//! its own action table and never redeclares them (UD3). Actions resolve in the key context
//! [`SHEET_CONTEXT`], which the canvas views set on the element that tracks their focus, so a
//! key reaches them only while the sheet holds GPUI focus. Their handlers are registered by
//! `tool::sheet_key_context`.
//!
//! The shell's keymap: the shell puts [`Rotate`], [`MirrorHorizontal`], [`MirrorVertical`] and
//! [`PlaceWire`] in its `ACTIONS` table with their R, X, Y and W bindings in `Sheet`, and
//! generates each binding exactly once from that table (U5.1). It registers nothing from this
//! module. It binds Del, Ctrl+Z and Ctrl+Y in `Workspace` to its own actions and Esc in
//! `Shell` (`NavCancel`), and its handlers call `SheetCanvas::undo`, `redo`, `cancel` and
//! (T5.3) `delete_selection` (UD3, UQ14). [`Cancel`], [`Delete`], [`Undo`] and [`Redo`] exist
//! for a canvas without the shell.
//!
//! [`standalone_bindings`] is that canvas's keymap (tests, bench and canvas harnesses): R, X,
//! Y, W, Esc, Del, Ctrl+Z and Ctrl+Y, all in `Sheet`. Never register it beside the shell's
//! keymap: R, X, Y and W would be bound twice, and Del, Ctrl+Z and Ctrl+Y twice on one context
//! stack (UINV4).

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

/// The keymap of a canvas without the shell: R, X, Y, W, Esc, Del, Ctrl+Z and Ctrl+Y, all in
/// `Sheet`. The shell never registers it (see the module docs).
pub fn standalone_bindings() -> Vec<KeyBinding> {
    let cx = Some(SHEET_CONTEXT);
    vec![
        KeyBinding::new("r", Rotate, cx),
        KeyBinding::new("x", MirrorHorizontal, cx),
        KeyBinding::new("y", MirrorVertical, cx),
        KeyBinding::new("w", PlaceWire, cx),
        KeyBinding::new("escape", Cancel, cx),
        KeyBinding::new("delete", Delete, cx),
        KeyBinding::new("ctrl-z", Undo, cx),
        KeyBinding::new("ctrl-y", Redo, cx),
    ]
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
    fn t5_2_standalone_bindings_are_the_eight_canvas_keys() {
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
