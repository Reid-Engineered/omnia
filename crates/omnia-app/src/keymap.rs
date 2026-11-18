//! Default keymap bindings, shortcut formatting, and UINV4 conflict validation (UD2, UD3, UINV4).

use crate::actions::{ACTIONS, FocusMenuBar, KeyContext};
use gpui::{KeyBinding, Keystroke, OperatingSystem, is_no_action, is_unbind};
use std::fmt::Write as _;

/// Every valid context stack allowed by the shell hierarchy (UD3).
///
/// Outermost is `Shell`, followed by `FirstBoot`, `Home`, or `Workspace`, then at most one leaf
/// (`Sheet`, `Panel`, `Instrument`, `Menu`, `Dialog`), with `TextField` optionally nested inside
/// `Dialog`, `Home`, `Panel`, or `Instrument`.
const VALID_CONTEXT_STACKS: &[&[KeyContext]] = &[
    &[KeyContext::Shell],
    &[KeyContext::Shell, KeyContext::FirstBoot],
    &[KeyContext::Shell, KeyContext::FirstBoot, KeyContext::Dialog],
    &[KeyContext::Shell, KeyContext::Home],
    &[KeyContext::Shell, KeyContext::Home, KeyContext::TextField],
    &[KeyContext::Shell, KeyContext::Home, KeyContext::Menu],
    &[KeyContext::Shell, KeyContext::Home, KeyContext::Dialog],
    &[
        KeyContext::Shell,
        KeyContext::Home,
        KeyContext::Dialog,
        KeyContext::TextField,
    ],
    &[KeyContext::Shell, KeyContext::Dialog],
    &[KeyContext::Shell, KeyContext::Dialog, KeyContext::TextField],
    &[KeyContext::Shell, KeyContext::Menu],
    &[KeyContext::Shell, KeyContext::Workspace],
    &[KeyContext::Shell, KeyContext::Workspace, KeyContext::Sheet],
    &[KeyContext::Shell, KeyContext::Workspace, KeyContext::Panel],
    &[
        KeyContext::Shell,
        KeyContext::Workspace,
        KeyContext::Panel,
        KeyContext::TextField,
    ],
    &[
        KeyContext::Shell,
        KeyContext::Workspace,
        KeyContext::Instrument,
    ],
    &[
        KeyContext::Shell,
        KeyContext::Workspace,
        KeyContext::Instrument,
        KeyContext::TextField,
    ],
    &[KeyContext::Shell, KeyContext::Workspace, KeyContext::Menu],
    &[KeyContext::Shell, KeyContext::Workspace, KeyContext::Dialog],
    &[
        KeyContext::Shell,
        KeyContext::Workspace,
        KeyContext::Dialog,
        KeyContext::TextField,
    ],
];

const UNMODIFIED_ALLOWED_CONTEXTS: &[KeyContext] = &[
    KeyContext::Sheet,
    KeyContext::Menu,
    KeyContext::Dialog,
    KeyContext::TextField,
];

/// Builds all default GPUI key bindings from [`ACTIONS`] and shell navigation keys.
///
/// Canvas tool bindings from `omnia_canvas::keymap` will be appended here when T5.2/U5.1 land.
pub fn default_bindings() -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    for spec in ACTIONS {
        for binding in spec.bindings {
            bindings.push(spec.key_binding(binding.keys, binding.context));
        }
    }
    bindings.push(KeyBinding::new(
        "f10",
        FocusMenuBar,
        Some(KeyContext::Shell.as_str()),
    ));
    bindings
}

/// Validates key bindings against UINV4 and returns `(keystroke, first, second)` conflict entries.
///
/// Checks two rules:
/// 1. No two active bindings share a keystroke (or chord prefix) in key contexts that can be
///    active together in [`VALID_CONTEXT_STACKS`].
/// 2. A letter, digit, or Space without a command modifier (`ctrl`, `alt`, `cmd`, `fn`) is bound
///    only in `Sheet`, `Menu`, `Dialog`, and `TextField`.
pub fn conflicts(bindings: &[KeyBinding]) -> Vec<(String, String, String)> {
    let parsed_stacks: Vec<Vec<gpui::KeyContext>> = VALID_CONTEXT_STACKS
        .iter()
        .map(|stack| {
            stack
                .iter()
                .map(|ctx| {
                    gpui::KeyContext::parse(ctx.as_str()).expect("KeyContext identifier must parse")
                })
                .collect()
        })
        .collect();

    let matches_stack = |binding: &KeyBinding, stack: &[gpui::KeyContext]| -> bool {
        match binding.predicate() {
            None => true,
            Some(pred) => pred.depth_of(stack).is_some(),
        }
    };

    let mut found = Vec::new();

    // UINV4 rule 2: unmodified letters, digits, and Space are restricted to leaf text/mode contexts.
    for binding in bindings {
        if is_no_action(binding.action()) || is_unbind(binding.action()) {
            continue;
        }
        let Some(first) = binding.keystrokes().first() else {
            continue;
        };
        if is_unmodified_letter_digit_or_space(first.inner()) {
            let matching_stacks: Vec<usize> = parsed_stacks
                .iter()
                .enumerate()
                .filter_map(|(idx, stack)| matches_stack(binding, stack).then_some(idx))
                .collect();

            let allowed = !matching_stacks.is_empty()
                && matching_stacks.iter().all(|&idx| {
                    VALID_CONTEXT_STACKS[idx]
                        .last()
                        .is_some_and(|leaf| UNMODIFIED_ALLOWED_CONTEXTS.contains(leaf))
                });

            if !allowed {
                found.push((
                    format_keybinding(binding, OperatingSystem::Windows),
                    describe_binding(binding),
                    "unmodified letter, digit, or Space is only allowed in Sheet, Menu, Dialog, or TextField".to_owned(),
                ));
            }
        }
    }

    // UINV4 rule 1: no two bindings share a keystroke (or chord prefix) in co-active contexts.
    for (i, a) in bindings.iter().enumerate() {
        if is_no_action(a.action()) || is_unbind(a.action()) {
            continue;
        }
        for b in &bindings[i + 1..] {
            if is_no_action(b.action()) || is_unbind(b.action()) {
                continue;
            }
            if !keystrokes_overlap(a, b) {
                continue;
            }

            let a_matches_any = parsed_stacks.iter().any(|stack| matches_stack(a, stack));
            let b_matches_any = parsed_stacks.iter().any(|stack| matches_stack(b, stack));

            let coactive = parsed_stacks
                .iter()
                .any(|stack| matches_stack(a, stack) && matches_stack(b, stack))
                || ((!a_matches_any || !b_matches_any) && a.predicate() == b.predicate());

            if coactive {
                found.push((
                    format_keybinding(a, OperatingSystem::Windows),
                    describe_binding(a),
                    describe_binding(b),
                ));
            }
        }
    }

    found
}

/// Formats a single [`Keystroke`] for display on `os` (e.g. `"Ctrl+S"`, `"Ctrl+Shift+S"`, `"F11"`).
pub fn format_keystroke(keystroke: &Keystroke, os: OperatingSystem) -> String {
    let raw = keystroke.unparse(os);
    format_unparsed_keystroke(&raw)
}

/// Formats a [`KeyBinding`] (supporting multi-keystroke chords) for display on `os`.
pub fn format_keybinding(binding: &KeyBinding, os: OperatingSystem) -> String {
    let mut out = String::new();
    for (idx, ks) in binding.keystrokes().iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let raw = ks.unparse(os);
        out.push_str(&format_unparsed_keystroke(&raw));
    }
    out
}

/// Formats a tooltip string `"Label (Shortcut)"` (UD3), or `"Label"` when `binding` is `None`.
pub fn tooltip_with_binding(
    label: &str,
    binding: Option<&KeyBinding>,
    os: OperatingSystem,
) -> String {
    match binding {
        Some(binding) => format!("{label} ({})", format_keybinding(binding, os)),
        None => label.to_owned(),
    }
}

/// Formats a tooltip string from a static [`crate::actions::ActionSpec`] (UD3).
pub fn tooltip_for_action(spec: &crate::actions::ActionSpec, os: OperatingSystem) -> String {
    let binding = spec
        .bindings
        .last()
        .map(|b| spec.key_binding(b.keys, b.context));
    tooltip_with_binding(spec.label, binding.as_ref(), os)
}

fn format_unparsed_keystroke(mut rest: &str) -> String {
    const MOD_PREFIXES: &[(&str, &str)] = &[
        ("fn-", "Fn"),
        ("ctrl-", "Ctrl"),
        ("alt-", "Alt"),
        ("win-", "Win"),
        ("super-", "Super"),
        ("cmd-", "Cmd"),
        ("shift-", "Shift"),
    ];

    let mut out = String::new();
    loop {
        let mut matched = false;
        for &(prefix, label) in MOD_PREFIXES {
            if let Some(next) = rest.strip_prefix(prefix) {
                if !out.is_empty() {
                    out.push('+');
                }
                out.push_str(label);
                rest = next;
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }

    if !out.is_empty() {
        out.push('+');
    }
    write_key_label(&mut out, rest);
    out
}

fn write_key_label(out: &mut String, key: &str) {
    match key {
        "space" => out.push_str("Space"),
        "tab" => out.push_str("Tab"),
        "enter" => out.push_str("Enter"),
        "escape" => out.push_str("Esc"),
        "backspace" => out.push_str("Backspace"),
        "delete" => out.push_str("Delete"),
        "insert" => out.push_str("Insert"),
        "home" => out.push_str("Home"),
        "end" => out.push_str("End"),
        "pageup" => out.push_str("PageUp"),
        "pagedown" => out.push_str("PageDown"),
        "up" => out.push_str("Up"),
        "down" => out.push_str("Down"),
        "left" => out.push_str("Left"),
        "right" => out.push_str("Right"),
        k if k.len() == 1 => {
            let ch = k.chars().next().expect("single char");
            out.push(ch.to_ascii_uppercase());
        }
        k if k.starts_with('f') && k.len() >= 2 && k[1..].bytes().all(|b| b.is_ascii_digit()) => {
            let _ = write!(out, "F{}", &k[1..]);
        }
        k => out.push_str(k),
    }
}

fn is_unmodified_letter_digit_or_space(keystroke: &Keystroke) -> bool {
    let has_command_modifier = keystroke.modifiers.control
        || keystroke.modifiers.alt
        || keystroke.modifiers.platform
        || keystroke.modifiers.function;
    if has_command_modifier {
        return false;
    }
    keystroke.key == "space"
        || (keystroke.key.len() == 1 && keystroke.key.as_bytes()[0].is_ascii_alphanumeric())
}

fn keystrokes_overlap(a: &KeyBinding, b: &KeyBinding) -> bool {
    let a_keys = a.keystrokes();
    let b_keys = b.keystrokes();
    if a_keys.is_empty() || b_keys.is_empty() {
        return false;
    }
    let min_len = a_keys.len().min(b_keys.len());
    a_keys[..min_len]
        .iter()
        .zip(&b_keys[..min_len])
        .all(|(ka, kb)| {
            ka.inner().modifiers == kb.inner().modifiers && ka.inner().key == kb.inner().key
        })
}

fn describe_binding(binding: &KeyBinding) -> String {
    let ctx = binding
        .predicate()
        .map_or_else(|| "<global>".to_owned(), |pred| pred.to_string());
    format!("{ctx}: {}", binding.action().name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{About, NewDocument, Quit, Rotate, Save, action_spec};
    use gpui::{
        Context, FocusHandle, InteractiveElement as _, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, div,
    };

    #[test]
    fn uinv4_default_bindings_have_no_conflicts_or_disallowed_bare_keys() {
        let bindings = default_bindings();
        assert!(!bindings.is_empty());
        let found = conflicts(&bindings);
        assert!(
            found.is_empty(),
            "UINV4 conflict(s) in default_bindings:\n{found:#?}"
        );
    }

    #[test]
    fn uinv4_planted_same_context_conflict_fails() {
        let mut bindings = default_bindings();
        bindings.push(KeyBinding::new(
            "ctrl-s",
            Quit,
            Some(KeyContext::Workspace.as_str()),
        ));
        let found = conflicts(&bindings);
        assert!(
            found.iter().any(|(k, _, _)| k == "Ctrl+S"),
            "expected planted same-context conflict on Ctrl+S, got {found:#?}"
        );
    }

    #[test]
    fn uinv4_planted_coactive_parent_child_context_conflict_fails() {
        let mut bindings = default_bindings();
        // `Shell` and `Workspace` are active together on `[Shell, Workspace]`.
        bindings.push(KeyBinding::new(
            "ctrl-s",
            Quit,
            Some(KeyContext::Shell.as_str()),
        ));
        let found = conflicts(&bindings);
        assert!(
            found
                .iter()
                .any(|(k, a, b)| k == "Ctrl+S" && a.contains("Workspace") && b.contains("Shell")),
            "expected co-active Shell/Workspace conflict on Ctrl+S, got {found:#?}"
        );

        let mut sheet_ws_bindings = default_bindings();
        // `Workspace` and `Sheet` are active together on `[Shell, Workspace, Sheet]`.
        sheet_ws_bindings.push(KeyBinding::new(
            "ctrl-z",
            Rotate,
            Some(KeyContext::Sheet.as_str()),
        ));
        let sheet_ws_found = conflicts(&sheet_ws_bindings);
        assert!(
            sheet_ws_found.iter().any(|(k, _, _)| k == "Ctrl+Z"),
            "expected co-active Workspace/Sheet conflict on Ctrl+Z, got {sheet_ws_found:#?}"
        );
    }

    #[test]
    fn uinv4_planted_global_context_conflict_fails() {
        let mut bindings = default_bindings();
        bindings.push(KeyBinding::new("f1", About, None));
        let found = conflicts(&bindings);
        assert!(
            found
                .iter()
                .any(|(k, _, b)| k == "F1" && b.contains("<global>")),
            "expected global conflict on F1, got {found:#?}"
        );
    }

    #[test]
    fn uinv4_planted_chord_prefix_conflict_fails() {
        let mut bindings = default_bindings();
        bindings.push(KeyBinding::new(
            "ctrl-n k",
            NewDocument,
            Some(KeyContext::Shell.as_str()),
        ));
        let found = conflicts(&bindings);
        assert!(
            found.iter().any(|(k, _, _)| k == "Ctrl+N"),
            "expected chord prefix conflict on Ctrl+N, got {found:#?}"
        );
    }

    #[test]
    fn uinv4_planted_unmodified_key_outside_allowed_contexts_fails() {
        for (keys, ctx) in [
            ("r", KeyContext::Workspace),
            ("R", KeyContext::Workspace),
            ("1", KeyContext::Shell),
            ("space", KeyContext::Panel),
            ("a", KeyContext::Home),
        ] {
            let mut bindings = default_bindings();
            bindings.push(KeyBinding::new(keys, Rotate, Some(ctx.as_str())));
            let found = conflicts(&bindings);
            assert!(
                !found.is_empty(),
                "expected UINV4 bare-key violation for {keys:?} in {ctx:?}"
            );
        }
    }

    #[test]
    fn uinv4_mutually_exclusive_leaf_contexts_do_not_conflict() {
        let bindings = vec![
            KeyBinding::new("escape", Rotate, Some(KeyContext::Sheet.as_str())),
            KeyBinding::new("escape", Quit, Some(KeyContext::Menu.as_str())),
            KeyBinding::new("escape", About, Some(KeyContext::Dialog.as_str())),
            KeyBinding::new("a", Rotate, Some(KeyContext::Sheet.as_str())),
            KeyBinding::new("a", Quit, Some(KeyContext::Menu.as_str())),
            KeyBinding::new("a", About, Some(KeyContext::TextField.as_str())),
            KeyBinding::new("space", Rotate, Some(KeyContext::Sheet.as_str())),
            KeyBinding::new("space", About, Some(KeyContext::Dialog.as_str())),
        ];
        let found = conflicts(&bindings);
        assert!(
            found.is_empty(),
            "mutually exclusive contexts should not conflict: {found:#?}"
        );
    }

    #[test]
    fn shortcut_formatting_and_tooltips_match_ud2_and_ud3() {
        for os in [OperatingSystem::Windows, OperatingSystem::Linux] {
            assert_eq!(
                tooltip_for_action(action_spec("file.save").unwrap(), os),
                "Save (Ctrl+S)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("file.save_as").unwrap(), os),
                "Save As... (Ctrl+Shift+S)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("view.fullscreen").unwrap(), os),
                "Full Screen (F11)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("view.zoom_in").unwrap(), os),
                "Zoom In (Ctrl+=)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("view.zoom_out").unwrap(), os),
                "Zoom Out (Ctrl+-)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("file.preferences").unwrap(), os),
                "Preferences... (Ctrl+,)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("window.next_tab").unwrap(), os),
                "Next Tab (Ctrl+Tab)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("window.previous_tab").unwrap(), os),
                "Previous Tab (Ctrl+Shift+Tab)"
            );
            assert_eq!(
                tooltip_for_action(action_spec("help.about").unwrap(), os),
                "About Omnia"
            );
        }
    }

    struct DispatchHarness {
        workspace_focus: FocusHandle,
        sheet_focus: FocusHandle,
        saves: usize,
        rotates: usize,
        quits: usize,
    }

    impl DispatchHarness {
        fn new(cx: &mut Context<Self>) -> Self {
            Self {
                workspace_focus: cx.focus_handle(),
                sheet_focus: cx.focus_handle(),
                saves: 0,
                rotates: 0,
                quits: 0,
            }
        }
    }

    impl Render for DispatchHarness {
        fn render(
            &mut self,
            _window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl gpui::IntoElement {
            div()
                .size_full()
                .key_context(KeyContext::Shell.as_str())
                .on_action(cx.listener(|this, _: &Quit, _, _| {
                    this.quits += 1;
                }))
                .child(
                    div()
                        .key_context(KeyContext::Workspace.as_str())
                        .track_focus(&self.workspace_focus)
                        .on_action(cx.listener(|this, _: &Save, _, _| {
                            this.saves += 1;
                        }))
                        .child(
                            div()
                                .key_context(KeyContext::Sheet.as_str())
                                .track_focus(&self.sheet_focus)
                                .on_action(cx.listener(|this, _: &Rotate, _, _| {
                                    this.rotates += 1;
                                })),
                        ),
                )
        }
    }

    #[gpui::test]
    fn gpui_binds_default_keymap_and_resolves_contexts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys(default_bindings());
        });

        let (view, cx) = cx.add_window_view(|_, cx| DispatchHarness::new(cx));

        // Focus `Workspace` (not `Sheet`): `ctrl-s` dispatches `Save`, `ctrl-q` dispatches `Quit`
        // (via ancestor `Shell`), while bare `r` does not dispatch `Rotate`.
        cx.update(|window, cx| {
            let focus = view.read(cx).workspace_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("ctrl-s r ctrl-q");

        cx.update(|window, cx| {
            let harness = view.read(cx);
            assert_eq!(harness.saves, 1);
            assert_eq!(harness.rotates, 0);
            assert_eq!(harness.quits, 1);

            let label = tooltip_with_binding(
                "Save",
                window.bindings_for_action(&Save).last(),
                OperatingSystem::Windows,
            );
            assert_eq!(label, "Save (Ctrl+S)");
        });

        // Focus `Sheet`: bare `r` now dispatches `Rotate` and `ctrl-s` still bubbles to `Workspace`.
        cx.update(|window, cx| {
            let focus = view.read(cx).sheet_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("r ctrl-s");

        cx.update(|_, cx| {
            let harness = view.read(cx);
            assert_eq!(harness.rotates, 1);
            assert_eq!(harness.saves, 2);
        });
    }
}
