//! Single action registry and per-window command state (UD3, UINV3, UINV5).
//!
//! Every user-visible menu item, toolbar button, instrument-strip item, tooltip shortcut,
//! and key binding is described once in [`ACTIONS`]. Canvas tool actions (`Rotate`,
//! `MirrorHorizontal`, `MirrorVertical`, `PlaceWire`, `PlaceNetLabel`) are listed here as
//! disabled placeholders until T5.2/U5.1 land; all [`CanvasEdits`] flags stay off until
//! their owning integration tasks.

use crate::theme::ThemeChoice;
use omnia_doc::SymbolId;

gpui::actions!(
    omnia,
    [
        NewDocument,
        Open,
        Save,
        SaveAs,
        Print,
        CloseTab,
        ShowHome,
        Preferences,
        Quit,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        Delete,
        SelectAll,
        Rotate,
        MirrorHorizontal,
        MirrorVertical,
        ZoomIn,
        ZoomOut,
        ZoomArea,
        ZoomFit,
        ToggleFullScreen,
        ToggleToolbox,
        ToggleSpreadsheet,
        ToggleResults,
        PlaceWire,
        PlaceNetLabel,
        TransientAnalysis,
        RunSimulation,
        PauseSimulation,
        StopSimulation,
        ToolsUnavailable,
        ReportsUnavailable,
        NextTab,
        PreviousTab,
        ShowShortcuts,
        About,
        FocusMenuBar,
    ]
);

#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = omnia, no_json)]
pub struct OpenRecent {
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = omnia, no_json)]
pub struct SetTheme {
    pub choice: ThemeChoice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstrumentKind {
    Multimeter,
    FunctionGenerator,
    Wattmeter,
    Oscilloscope,
    FourChannelScope,
    BodePlotter,
    FrequencyCounter,
    WordGenerator,
    LogicAnalyzer,
    LogicConverter,
    IvAnalyzer,
    DistortionAnalyzer,
    SpectrumAnalyzer,
    NetworkAnalyzer,
    Probe,
}

impl InstrumentKind {
    pub const ALL: [Self; 15] = [
        Self::Multimeter,
        Self::FunctionGenerator,
        Self::Wattmeter,
        Self::Oscilloscope,
        Self::FourChannelScope,
        Self::BodePlotter,
        Self::FrequencyCounter,
        Self::WordGenerator,
        Self::LogicAnalyzer,
        Self::LogicConverter,
        Self::IvAnalyzer,
        Self::DistortionAnalyzer,
        Self::SpectrumAnalyzer,
        Self::NetworkAnalyzer,
        Self::Probe,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = omnia, no_json)]
pub struct ToggleInstrument {
    pub kind: InstrumentKind,
}

#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = omnia, no_json)]
pub struct PlacePart {
    pub symbol: SymbolId,
}

/// Top-level menu bar entries in UK2 order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TopMenu {
    File,
    Edit,
    View,
    Place,
    Simulate,
    Tools,
    Reports,
    Window,
    Help,
}

impl TopMenu {
    pub const ALL: [Self; 9] = [
        Self::File,
        Self::Edit,
        Self::View,
        Self::Place,
        Self::Simulate,
        Self::Tools,
        Self::Reports,
        Self::Window,
        Self::Help,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Place => "Place",
            Self::Simulate => "Simulate",
            Self::Tools => "Tools",
            Self::Reports => "Reports",
            Self::Window => "Window",
            Self::Help => "Help",
        }
    }
}

/// Key contexts in the shell hierarchy (UD3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyContext {
    Shell,
    Workspace,
    Sheet,
    Panel,
    Instrument,
    Menu,
    Dialog,
    Home,
    FirstBoot,
    TextField,
}

impl KeyContext {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "Shell",
            Self::Workspace => "Workspace",
            Self::Sheet => "Sheet",
            Self::Panel => "Panel",
            Self::Instrument => "Instrument",
            Self::Menu => "Menu",
            Self::Dialog => "Dialog",
            Self::Home => "Home",
            Self::FirstBoot => "FirstBoot",
            Self::TextField => "TextField",
        }
    }
}

/// Static key binding declaration attached to an [`ActionSpec`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static str,
    pub context: KeyContext,
}

/// Where an action appears in the menu bar (UD2, UINV3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuPlace {
    None,
    At {
        menu: TopMenu,
        group: u8,
        order: u8,
    },
    Submenu {
        menu: TopMenu,
        submenu: &'static str,
        group: u8,
        order: u8,
    },
}

/// Toolbar groups in UK3 order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolbarGroup {
    File,
    Edit,
    Zoom,
    Sim,
}

/// Where an action appears in the workspace toolbar (UK3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolbarPlace {
    None,
    At { group: ToolbarGroup, order: u8 },
}

/// Active screen in `Shell` (UD4, UD15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenKind {
    FirstBoot,
    Home,
    #[default]
    Workspace,
}

/// Feature gates for canvas editing methods that arrive in U2.5 / U5.x (UD3, section 10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanvasEdits {
    pub undo: bool,
    pub delete: bool,
    pub clipboard: bool,
    pub place: bool,
}

/// Simulation lifecycle state (UD19). Stage 2 stays [`SimState::Unavailable`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SimState {
    #[default]
    Unavailable,
    Idle,
    Running,
    Paused,
    Failed,
}

/// Per-window enabled/checked state folded from document and canvas events (UD3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandState {
    pub screen: ScreenKind,
    pub has_document: bool,
    pub tab_count: usize,
    pub loading: bool,
    pub modified: bool,
    pub untitled: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub has_clip: bool,
    pub canvas_edits: CanvasEdits,
    pub toolbox: bool,
    pub spreadsheet: bool,
    pub results: bool,
    pub full_screen: bool,
    pub sim: SimState,
    pub theme: ThemeChoice,
}

impl CommandState {
    /// Whether the workspace has an active, loaded document ready for commands.
    pub const fn is_ready_doc(&self) -> bool {
        matches!(self.screen, ScreenKind::Workspace) && self.has_document && !self.loading
    }
}

/// Static specification of a registered user command (UD3).
pub struct ActionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: Option<&'static str>,
    pub menu: MenuPlace,
    pub toolbar: ToolbarPlace,
    pub bindings: &'static [Binding],
    pub build: fn() -> Box<dyn gpui::Action>,
    pub bind: fn(&'static str, Option<&str>) -> gpui::KeyBinding,
    pub enabled: fn(&CommandState) -> bool,
    pub checked: fn(&CommandState) -> bool,
}

impl ActionSpec {
    /// Builds a GPUI [`gpui::KeyBinding`] for this action in `context`.
    pub fn key_binding(&self, keys: &'static str, context: KeyContext) -> gpui::KeyBinding {
        (self.bind)(keys, Some(context.as_str()))
    }
}

macro_rules! action_spec {
    (
        id: $id:expr,
        label: $label:expr,
        icon: $icon:expr,
        menu: $menu:expr,
        toolbar: $toolbar:expr,
        bindings: $bindings:expr,
        action: $action:expr,
        enabled: $enabled:expr,
        checked: $checked:expr $(,)?
    ) => {
        ActionSpec {
            id: $id,
            label: $label,
            icon: $icon,
            menu: $menu,
            toolbar: $toolbar,
            bindings: $bindings,
            build: || Box::new($action),
            bind: |keys, ctx| gpui::KeyBinding::new(keys, $action, ctx),
            enabled: $enabled,
            checked: $checked,
        }
    };
}

const fn b(keys: &'static str, context: KeyContext) -> Binding {
    Binding { keys, context }
}

fn always(_: &CommandState) -> bool {
    true
}

fn never(_: &CommandState) -> bool {
    false
}

fn not_first_boot(s: &CommandState) -> bool {
    s.screen != ScreenKind::FirstBoot
}

fn in_workspace(s: &CommandState) -> bool {
    s.screen == ScreenKind::Workspace
}

fn ready_doc(s: &CommandState) -> bool {
    s.is_ready_doc()
}

/// Enabled predicate for dynamic `PlacePart` items (U4.2, U5.2).
pub fn place_part_enabled(s: &CommandState) -> bool {
    s.is_ready_doc() && s.canvas_edits.place
}

/// Enabled predicate for dynamic `OpenRecent` items (U2.3).
pub fn open_recent_enabled(s: &CommandState) -> bool {
    not_first_boot(s)
}

/// The stage 2 action registry (UD2, UD3, UK3).
pub static ACTIONS: &[ActionSpec] = &[
    // ---- File ----
    action_spec! {
        id: "file.new",
        label: "New",
        icon: Some("file-plus"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::File,
            order: 0,
        },
        bindings: &[b("ctrl-n", KeyContext::Shell)],
        action: NewDocument,
        enabled: not_first_boot,
        checked: never,
    },
    action_spec! {
        id: "file.open",
        label: "Open...",
        icon: Some("folder-open"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 0,
            order: 1,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::File,
            order: 1,
        },
        bindings: &[b("ctrl-o", KeyContext::Shell)],
        action: Open,
        enabled: not_first_boot,
        checked: never,
    },
    action_spec! {
        id: "file.save",
        label: "Save",
        icon: Some("device-floppy"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 1,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::File,
            order: 2,
        },
        bindings: &[b("ctrl-s", KeyContext::Workspace)],
        action: Save,
        enabled: |s| s.is_ready_doc() && (s.modified || s.untitled),
        checked: never,
    },
    action_spec! {
        id: "file.save_as",
        label: "Save As...",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 1,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-shift-s", KeyContext::Workspace)],
        action: SaveAs,
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "file.print",
        label: "Print...",
        icon: Some("printer"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 1,
            order: 2,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::File,
            order: 3,
        },
        bindings: &[],
        action: Print,
        enabled: never,
        checked: never,
    },
    action_spec! {
        id: "file.close_tab",
        label: "Close Tab",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 1,
            order: 3,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-w", KeyContext::Workspace)],
        action: CloseTab,
        enabled: |s| s.screen == ScreenKind::Workspace && s.has_document,
        checked: never,
    },
    action_spec! {
        id: "file.home",
        label: "Home",
        icon: Some("home"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 2,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-shift-h", KeyContext::Workspace)],
        action: ShowHome,
        enabled: in_workspace,
        checked: never,
    },
    action_spec! {
        id: "file.preferences",
        label: "Preferences...",
        icon: Some("settings"),
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 2,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-,", KeyContext::Shell)],
        action: Preferences,
        enabled: not_first_boot,
        checked: never,
    },
    action_spec! {
        id: "file.quit",
        label: "Quit",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::File,
            group: 3,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-q", KeyContext::Shell)],
        action: Quit,
        enabled: always,
        checked: never,
    },
    // ---- Edit ----
    action_spec! {
        id: "edit.undo",
        label: "Undo",
        icon: Some("arrow-back-up"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Edit,
            order: 0,
        },
        bindings: &[b("ctrl-z", KeyContext::Workspace)],
        action: Undo,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.undo && s.can_undo,
        checked: never,
    },
    action_spec! {
        id: "edit.redo",
        label: "Redo",
        icon: Some("arrow-forward-up"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 0,
            order: 1,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Edit,
            order: 1,
        },
        bindings: &[
            b("ctrl-shift-z", KeyContext::Workspace),
            b("ctrl-y", KeyContext::Workspace),
        ],
        action: Redo,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.undo && s.can_redo,
        checked: never,
    },
    action_spec! {
        id: "edit.cut",
        label: "Cut",
        icon: Some("scissors"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 1,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Edit,
            order: 2,
        },
        bindings: &[b("ctrl-x", KeyContext::Workspace)],
        action: Cut,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.clipboard && s.has_selection,
        checked: never,
    },
    action_spec! {
        id: "edit.copy",
        label: "Copy",
        icon: Some("copy"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 1,
            order: 1,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Edit,
            order: 3,
        },
        bindings: &[b("ctrl-c", KeyContext::Workspace)],
        action: Copy,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.clipboard && s.has_selection,
        checked: never,
    },
    action_spec! {
        id: "edit.paste",
        label: "Paste",
        icon: Some("clipboard"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 1,
            order: 2,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Edit,
            order: 4,
        },
        bindings: &[b("ctrl-v", KeyContext::Workspace)],
        action: Paste,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.clipboard && s.has_clip,
        checked: never,
    },
    action_spec! {
        id: "edit.delete",
        label: "Delete",
        icon: Some("trash"),
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 1,
            order: 3,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("delete", KeyContext::Workspace)],
        action: Delete,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.delete && s.has_selection,
        checked: never,
    },
    action_spec! {
        id: "edit.select_all",
        label: "Select All",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 2,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-a", KeyContext::Workspace)],
        action: SelectAll,
        enabled: |s| s.is_ready_doc() && s.canvas_edits.delete,
        checked: never,
    },
    // Canvas tool placeholders (replaced by T5.2 actions in U5.1).
    action_spec! {
        id: "edit.rotate",
        label: "Rotate",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 3,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("r", KeyContext::Sheet)],
        action: Rotate,
        enabled: never,
        checked: never,
    },
    action_spec! {
        id: "edit.mirror_horizontal",
        label: "Mirror Horizontal",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 3,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("x", KeyContext::Sheet)],
        action: MirrorHorizontal,
        enabled: never,
        checked: never,
    },
    action_spec! {
        id: "edit.mirror_vertical",
        label: "Mirror Vertical",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Edit,
            group: 3,
            order: 2,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("y", KeyContext::Sheet)],
        action: MirrorVertical,
        enabled: never,
        checked: never,
    },
    // ---- View ----
    action_spec! {
        id: "view.zoom_in",
        label: "Zoom In",
        icon: Some("zoom-in"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Zoom,
            order: 0,
        },
        bindings: &[b("ctrl-=", KeyContext::Workspace)],
        action: ZoomIn,
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "view.zoom_out",
        label: "Zoom Out",
        icon: Some("zoom-out"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 0,
            order: 1,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Zoom,
            order: 1,
        },
        bindings: &[b("ctrl--", KeyContext::Workspace)],
        action: ZoomOut,
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "view.zoom_area",
        label: "Zoom Area",
        icon: Some("zoom-in-area"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 0,
            order: 2,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Zoom,
            order: 2,
        },
        bindings: &[],
        action: ZoomArea,
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "view.zoom_fit",
        label: "Fit",
        icon: Some("arrows-maximize"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 0,
            order: 3,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Zoom,
            order: 3,
        },
        bindings: &[b("ctrl-0", KeyContext::Workspace)],
        action: ZoomFit,
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "view.fullscreen",
        label: "Full Screen",
        icon: Some("maximize"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 0,
            order: 4,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Zoom,
            order: 4,
        },
        bindings: &[b("f11", KeyContext::Shell)],
        action: ToggleFullScreen,
        enabled: always,
        checked: |s| s.full_screen,
    },
    action_spec! {
        id: "view.toolbox",
        label: "Toolbox",
        icon: Some("layout-sidebar"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 1,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-1", KeyContext::Workspace)],
        action: ToggleToolbox,
        enabled: in_workspace,
        checked: |s| s.toolbox,
    },
    action_spec! {
        id: "view.spreadsheet",
        label: "Spreadsheet",
        icon: Some("table"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 1,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-2", KeyContext::Workspace)],
        action: ToggleSpreadsheet,
        enabled: in_workspace,
        checked: |s| s.spreadsheet,
    },
    action_spec! {
        id: "view.results",
        label: "Results",
        icon: Some("layout-bottombar"),
        menu: MenuPlace::At {
            menu: TopMenu::View,
            group: 1,
            order: 2,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-3", KeyContext::Workspace)],
        action: ToggleResults,
        enabled: in_workspace,
        checked: |s| s.results,
    },
    action_spec! {
        id: "view.theme.graphite",
        label: "Graphite",
        icon: None,
        menu: MenuPlace::Submenu {
            menu: TopMenu::View,
            submenu: "Theme",
            group: 2,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: SetTheme {
            choice: ThemeChoice::Graphite,
        },
        enabled: not_first_boot,
        checked: |s| s.theme == ThemeChoice::Graphite,
    },
    action_spec! {
        id: "view.theme.gray",
        label: "Gray",
        icon: None,
        menu: MenuPlace::Submenu {
            menu: TopMenu::View,
            submenu: "Theme",
            group: 2,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: SetTheme {
            choice: ThemeChoice::Gray,
        },
        enabled: not_first_boot,
        checked: |s| s.theme == ThemeChoice::Gray,
    },
    action_spec! {
        id: "view.theme.system",
        label: "Match System",
        icon: None,
        menu: MenuPlace::Submenu {
            menu: TopMenu::View,
            submenu: "Theme",
            group: 2,
            order: 2,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: SetTheme {
            choice: ThemeChoice::System,
        },
        enabled: not_first_boot,
        checked: |s| s.theme == ThemeChoice::System,
    },
    // ---- Place ----
    action_spec! {
        id: "place.wire",
        label: "Wire",
        icon: Some("route"),
        menu: MenuPlace::At {
            menu: TopMenu::Place,
            group: 1,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("w", KeyContext::Sheet)],
        action: PlaceWire,
        enabled: never,
        checked: never,
    },
    action_spec! {
        id: "place.net_label",
        label: "Net Label",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Place,
            group: 1,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: PlaceNetLabel,
        enabled: never,
        checked: never,
    },
    // ---- Simulate (all disabled in stage 2, UD19) ----
    action_spec! {
        id: "simulate.transient",
        label: "Transient",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Simulate,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: TransientAnalysis,
        enabled: |s| s.is_ready_doc() && s.sim != SimState::Unavailable,
        checked: never,
    },
    action_spec! {
        id: "simulate.run",
        label: "Run",
        icon: Some("player-play"),
        menu: MenuPlace::At {
            menu: TopMenu::Simulate,
            group: 1,
            order: 0,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Sim,
            order: 0,
        },
        bindings: &[],
        action: RunSimulation,
        enabled: |s| {
            s.is_ready_doc()
                && matches!(
                    s.sim,
                    SimState::Idle | SimState::Paused | SimState::Failed
                )
        },
        checked: |s| s.sim == SimState::Running,
    },
    action_spec! {
        id: "simulate.pause",
        label: "Pause",
        icon: Some("player-pause"),
        menu: MenuPlace::At {
            menu: TopMenu::Simulate,
            group: 1,
            order: 1,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Sim,
            order: 1,
        },
        bindings: &[],
        action: PauseSimulation,
        enabled: |s| s.is_ready_doc() && s.sim == SimState::Running,
        checked: |s| s.sim == SimState::Paused,
    },
    action_spec! {
        id: "simulate.stop",
        label: "Stop",
        icon: Some("player-stop"),
        menu: MenuPlace::At {
            menu: TopMenu::Simulate,
            group: 1,
            order: 2,
        },
        toolbar: ToolbarPlace::At {
            group: ToolbarGroup::Sim,
            order: 2,
        },
        bindings: &[],
        action: StopSimulation,
        enabled: |s| s.is_ready_doc() && matches!(s.sim, SimState::Running | SimState::Paused),
        checked: never,
    },
    // ---- Tools / Reports (UD2 placeholders) ----
    action_spec! {
        id: "tools.unavailable",
        label: "Not available yet",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Tools,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToolsUnavailable,
        enabled: never,
        checked: never,
    },
    action_spec! {
        id: "reports.unavailable",
        label: "Not available yet",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Reports,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ReportsUnavailable,
        enabled: never,
        checked: never,
    },
    // ---- Window ----
    action_spec! {
        id: "window.next_tab",
        label: "Next Tab",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Window,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-tab", KeyContext::Workspace)],
        action: NextTab,
        enabled: |s| s.screen == ScreenKind::Workspace && s.tab_count > 1,
        checked: never,
    },
    action_spec! {
        id: "window.previous_tab",
        label: "Previous Tab",
        icon: None,
        menu: MenuPlace::At {
            menu: TopMenu::Window,
            group: 0,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("ctrl-shift-tab", KeyContext::Workspace)],
        action: PreviousTab,
        enabled: |s| s.screen == ScreenKind::Workspace && s.tab_count > 1,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.multimeter",
        label: "Multimeter",
        icon: Some("gauge"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::Multimeter,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.function_generator",
        label: "Function Generator",
        icon: Some("wave-sine"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::FunctionGenerator,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.wattmeter",
        label: "Wattmeter",
        icon: Some("bolt"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 2,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::Wattmeter,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.oscilloscope",
        label: "Oscilloscope",
        icon: Some("device-desktop-analytics"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 3,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::Oscilloscope,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.four_channel_scope",
        label: "4-Channel Scope",
        icon: Some("chart-line"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 4,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::FourChannelScope,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.bode_plotter",
        label: "Bode Plotter",
        icon: Some("chart-ppf"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 5,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::BodePlotter,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.frequency_counter",
        label: "Frequency Counter",
        icon: Some("stopwatch"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 6,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::FrequencyCounter,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.word_generator",
        label: "Word Generator",
        icon: Some("binary"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 7,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::WordGenerator,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.logic_analyzer",
        label: "Logic Analyzer",
        icon: Some("wave-square"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 8,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::LogicAnalyzer,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.logic_converter",
        label: "Logic Converter",
        icon: Some("arrows-exchange"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 9,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::LogicConverter,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.iv_analyzer",
        label: "IV Analyzer",
        icon: Some("chart-dots"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 10,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::IvAnalyzer,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.distortion_analyzer",
        label: "Distortion Analyzer",
        icon: Some("wave-saw-tool"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 11,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::DistortionAnalyzer,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.spectrum_analyzer",
        label: "Spectrum Analyzer",
        icon: Some("chart-histogram"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 12,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::SpectrumAnalyzer,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.network_analyzer",
        label: "Network Analyzer",
        icon: Some("network"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 13,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::NetworkAnalyzer,
        },
        enabled: ready_doc,
        checked: never,
    },
    action_spec! {
        id: "window.instrument.probe",
        label: "Probe",
        icon: Some("needle"),
        menu: MenuPlace::Submenu {
            menu: TopMenu::Window,
            submenu: "Instruments",
            group: 2,
            order: 14,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: ToggleInstrument {
            kind: InstrumentKind::Probe,
        },
        enabled: ready_doc,
        checked: never,
    },
    // ---- Help ----
    action_spec! {
        id: "help.shortcuts",
        label: "Keyboard Shortcuts",
        icon: Some("keyboard"),
        menu: MenuPlace::At {
            menu: TopMenu::Help,
            group: 0,
            order: 0,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[b("f1", KeyContext::Shell)],
        action: ShowShortcuts,
        enabled: not_first_boot,
        checked: never,
    },
    action_spec! {
        id: "help.about",
        label: "About Omnia",
        icon: Some("info-circle"),
        menu: MenuPlace::At {
            menu: TopMenu::Help,
            group: 0,
            order: 1,
        },
        toolbar: ToolbarPlace::None,
        bindings: &[],
        action: About,
        enabled: not_first_boot,
        checked: never,
    },
];

/// Looks up an action specification by stable `id` (e.g. `"file.save"`).
pub fn action_spec(id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|spec| spec.id == id)
}

/// Returns all registered actions belonging to `menu`, sorted by `(group, order)`.
pub fn menu_actions(menu: TopMenu) -> Vec<&'static ActionSpec> {
    let mut items: Vec<&'static ActionSpec> = ACTIONS
        .iter()
        .filter(|spec| match spec.menu {
            MenuPlace::At { menu: m, .. } | MenuPlace::Submenu { menu: m, .. } => m == menu,
            MenuPlace::None => false,
        })
        .collect();
    items.sort_by_key(|spec| match spec.menu {
        MenuPlace::At { group, order, .. } | MenuPlace::Submenu { group, order, .. } => {
            (group, order)
        }
        MenuPlace::None => (u8::MAX, u8::MAX),
    });
    items
}

/// Returns all registered actions belonging to `group` in the toolbar, sorted by `order`.
pub fn toolbar_actions(group: ToolbarGroup) -> Vec<&'static ActionSpec> {
    let mut items: Vec<&'static ActionSpec> = ACTIONS
        .iter()
        .filter(|spec| matches!(spec.toolbar, ToolbarPlace::At { group: g, .. } if g == group))
        .collect();
    items.sort_by_key(|spec| match spec.toolbar {
        ToolbarPlace::At { order, .. } => order,
        ToolbarPlace::None => u8::MAX,
    });
    items
}

/// Returns all registered instrument actions in strip order.
pub fn instrument_actions() -> Vec<&'static ActionSpec> {
    let mut items: Vec<&'static ActionSpec> = ACTIONS
        .iter()
        .filter(|spec| spec.id.starts_with("window.instrument."))
        .collect();
    items.sort_by_key(|spec| match spec.menu {
        MenuPlace::Submenu { order, .. } | MenuPlace::At { order, .. } => order,
        MenuPlace::None => u8::MAX,
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets;
    use std::collections::HashSet;

    fn ready_workspace() -> CommandState {
        CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 1,
            ..CommandState::default()
        }
    }

    #[test]
    fn uinv3_every_action_is_in_a_menu_and_unique() {
        let mut ids = HashSet::new();
        let mut menu_slots = HashSet::new();
        let mut toolbar_slots = HashSet::new();
        let mut built: Vec<(&'static str, Box<dyn gpui::Action>)> = Vec::new();

        for spec in ACTIONS {
            assert!(!spec.id.is_empty());
            assert!(!spec.label.is_empty());
            assert!(ids.insert(spec.id), "duplicate action id: {}", spec.id);

            match spec.menu {
                MenuPlace::None => {
                    panic!(
                        "UINV3: action {} has MenuPlace::None and is unreachable from menus",
                        spec.id
                    );
                }
                MenuPlace::At { menu, group, order } => {
                    assert!(
                        menu_slots.insert((menu, "", group, order)),
                        "duplicate menu slot ({menu:?}, {group}, {order}) on {}",
                        spec.id
                    );
                }
                MenuPlace::Submenu {
                    menu,
                    submenu,
                    group,
                    order,
                } => {
                    assert!(!submenu.is_empty());
                    assert!(
                        menu_slots.insert((menu, submenu, group, order)),
                        "duplicate submenu slot ({menu:?}, {submenu}, {group}, {order}) on {}",
                        spec.id
                    );
                }
            }

            if let ToolbarPlace::At { group, order } = spec.toolbar {
                assert!(
                    spec.icon.is_some(),
                    "toolbar action {} must specify an icon",
                    spec.id
                );
                assert!(
                    toolbar_slots.insert((group, order)),
                    "duplicate toolbar slot ({group:?}, {order}) on {}",
                    spec.id
                );
            }

            if let Some(icon) = spec.icon {
                assert!(
                    assets::icon_bytes(icon).is_some(),
                    "action {} references missing icon {icon:?}",
                    spec.id
                );
            }

            let action = (spec.build)();
            for (prev_id, prev_action) in &built {
                assert!(
                    !action.partial_eq(prev_action.as_ref()),
                    "actions {prev_id} and {} build equal gpui::Action values",
                    spec.id
                );
            }
            built.push((spec.id, action));
        }
    }

    #[test]
    fn uinv3_every_menu_and_toolbar_group_is_populated_in_order() {
        assert_eq!(TopMenu::ALL.len(), 9);
        for menu in TopMenu::ALL {
            assert!(
                !menu_actions(menu).is_empty(),
                "menu {:?} has no actions",
                menu
            );
        }
        assert_eq!(
            toolbar_actions(ToolbarGroup::File)
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            ["file.new", "file.open", "file.save", "file.print"]
        );
        assert_eq!(
            toolbar_actions(ToolbarGroup::Edit)
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            [
                "edit.undo",
                "edit.redo",
                "edit.cut",
                "edit.copy",
                "edit.paste"
            ]
        );
        assert_eq!(
            toolbar_actions(ToolbarGroup::Zoom)
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            [
                "view.zoom_in",
                "view.zoom_out",
                "view.zoom_area",
                "view.zoom_fit",
                "view.fullscreen"
            ]
        );
        assert_eq!(
            toolbar_actions(ToolbarGroup::Sim)
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            ["simulate.run", "simulate.pause", "simulate.stop"]
        );
        assert_eq!(instrument_actions().len(), InstrumentKind::ALL.len());
    }

    #[test]
    fn canvas_edit_flags_default_to_off_and_gate_edits() {
        let state = CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 2,
            modified: true,
            can_undo: true,
            can_redo: true,
            has_selection: true,
            has_clip: true,
            ..CommandState::default()
        };
        assert_eq!(state.canvas_edits, CanvasEdits::default());
        assert!(!state.canvas_edits.undo);
        assert!(!state.canvas_edits.delete);
        assert!(!state.canvas_edits.clipboard);
        assert!(!state.canvas_edits.place);
        assert!(!place_part_enabled(&state));

        for id in [
            "edit.undo",
            "edit.redo",
            "edit.cut",
            "edit.copy",
            "edit.paste",
            "edit.delete",
            "edit.select_all",
            "edit.rotate",
            "edit.mirror_horizontal",
            "edit.mirror_vertical",
            "place.wire",
            "place.net_label",
        ] {
            let spec = action_spec(id).unwrap();
            assert!(
                !(spec.enabled)(&state),
                "{id} must stay disabled while canvas_edits flags are off"
            );
        }
    }

    #[test]
    fn representative_enabled_and_checked_table_for_every_action() {
        let first_boot = CommandState {
            screen: ScreenKind::FirstBoot,
            ..CommandState::default()
        };
        let home = CommandState {
            screen: ScreenKind::Home,
            ..CommandState::default()
        };
        let empty_ws = CommandState {
            screen: ScreenKind::Workspace,
            has_document: false,
            ..CommandState::default()
        };
        let loading_ws = CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 1,
            loading: true,
            modified: true,
            can_undo: true,
            has_selection: true,
            canvas_edits: CanvasEdits {
                undo: true,
                delete: true,
                clipboard: true,
                place: true,
            },
            ..CommandState::default()
        };
        let untitled_clean = CommandState {
            untitled: true,
            ..ready_workspace()
        };
        let saved_clean = ready_workspace();
        let modified_titled = CommandState {
            modified: true,
            ..ready_workspace()
        };
        let all_edits_on = CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 2,
            modified: true,
            can_undo: true,
            can_redo: true,
            has_selection: true,
            has_clip: true,
            canvas_edits: CanvasEdits {
                undo: true,
                delete: true,
                clipboard: true,
                place: true,
            },
            toolbox: true,
            spreadsheet: true,
            results: true,
            full_screen: true,
            sim: SimState::Running,
            theme: ThemeChoice::Gray,
            ..CommandState::default()
        };
        let sim_idle = CommandState {
            sim: SimState::Idle,
            ..ready_workspace()
        };

        let states: &[(&str, &CommandState)] = &[
            ("first_boot", &first_boot),
            ("home", &home),
            ("empty_ws", &empty_ws),
            ("loading_ws", &loading_ws),
            ("untitled_clean", &untitled_clean),
            ("saved_clean", &saved_clean),
            ("modified_titled", &modified_titled),
            ("all_edits_on", &all_edits_on),
            ("sim_idle", &sim_idle),
        ];

        // Columns match `states` order above.
        let expected_enabled: &[(&str, [bool; 9])] = &[
            (
                "file.new",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "file.open",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "file.save",
                [false, false, false, false, true, false, true, true, false],
            ),
            (
                "file.save_as",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "file.print",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "file.close_tab",
                [false, false, false, true, true, true, true, true, true],
            ),
            (
                "file.home",
                [false, false, true, true, true, true, true, true, true],
            ),
            (
                "file.preferences",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "file.quit",
                [true, true, true, true, true, true, true, true, true],
            ),
            (
                "edit.undo",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.redo",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.cut",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.copy",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.paste",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.delete",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.select_all",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "edit.rotate",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "edit.mirror_horizontal",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "edit.mirror_vertical",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "view.zoom_in",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "view.zoom_out",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "view.zoom_area",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "view.zoom_fit",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "view.fullscreen",
                [true, true, true, true, true, true, true, true, true],
            ),
            (
                "view.toolbox",
                [false, false, true, true, true, true, true, true, true],
            ),
            (
                "view.spreadsheet",
                [false, false, true, true, true, true, true, true, true],
            ),
            (
                "view.results",
                [false, false, true, true, true, true, true, true, true],
            ),
            (
                "view.theme.graphite",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "view.theme.gray",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "view.theme.system",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "place.wire",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "place.net_label",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "simulate.transient",
                [false, false, false, false, false, false, false, true, true],
            ),
            (
                "simulate.run",
                [false, false, false, false, false, false, false, false, true],
            ),
            (
                "simulate.pause",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "simulate.stop",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "tools.unavailable",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "reports.unavailable",
                [
                    false, false, false, false, false, false, false, false, false,
                ],
            ),
            (
                "window.next_tab",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "window.previous_tab",
                [false, false, false, false, false, false, false, true, false],
            ),
            (
                "window.instrument.multimeter",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.function_generator",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.wattmeter",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.oscilloscope",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.four_channel_scope",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.bode_plotter",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.frequency_counter",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.word_generator",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.logic_analyzer",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.logic_converter",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.iv_analyzer",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.distortion_analyzer",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.spectrum_analyzer",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.network_analyzer",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "window.instrument.probe",
                [false, false, false, false, true, true, true, true, true],
            ),
            (
                "help.shortcuts",
                [false, true, true, true, true, true, true, true, true],
            ),
            (
                "help.about",
                [false, true, true, true, true, true, true, true, true],
            ),
        ];

        assert_eq!(
            expected_enabled.len(),
            ACTIONS.len(),
            "table test must cover every action in ACTIONS"
        );

        for (id, expected) in expected_enabled {
            let spec = action_spec(id).unwrap_or_else(|| panic!("missing action {id}"));
            for ((state_name, state), &want) in states.iter().zip(expected.iter()) {
                assert_eq!(
                    (spec.enabled)(state),
                    want,
                    "enabled mismatch for {id} in state {state_name}"
                );
            }
        }

        // Checked state assertions.
        assert!((action_spec("view.theme.graphite").unwrap().checked)(
            &saved_clean
        ));
        assert!(!(action_spec("view.theme.gray").unwrap().checked)(
            &saved_clean
        ));
        assert!((action_spec("view.theme.gray").unwrap().checked)(
            &all_edits_on
        ));
        assert!((action_spec("view.toolbox").unwrap().checked)(
            &all_edits_on
        ));
        assert!((action_spec("view.spreadsheet").unwrap().checked)(
            &all_edits_on
        ));
        assert!((action_spec("view.results").unwrap().checked)(
            &all_edits_on
        ));
        assert!((action_spec("view.fullscreen").unwrap().checked)(
            &all_edits_on
        ));
        assert!((action_spec("simulate.run").unwrap().checked)(
            &all_edits_on
        ));
        assert!(!(action_spec("simulate.pause").unwrap().checked)(
            &all_edits_on
        ));
        let sim_paused = CommandState {
            sim: SimState::Paused,
            ..ready_workspace()
        };
        assert!((action_spec("simulate.pause").unwrap().checked)(
            &sim_paused
        ));
        assert!(!(action_spec("view.toolbox").unwrap().checked)(
            &saved_clean
        ));
        assert!(open_recent_enabled(&home));
        assert!(!open_recent_enabled(&first_boot));
        assert!(place_part_enabled(&all_edits_on));
        assert!(!place_part_enabled(&saved_clean));
    }

    #[test]
    fn uinv3_mouse_reachable_and_parameterized_actions_are_well_formed() {
        use gpui::Action as _;

        // Every toolbar and instrument-strip item is mouse-reachable and must have a menu path
        // or a binding (UINV3).
        for group in [
            ToolbarGroup::File,
            ToolbarGroup::Edit,
            ToolbarGroup::Zoom,
            ToolbarGroup::Sim,
        ] {
            for spec in toolbar_actions(group) {
                assert!(action_spec(spec.id).is_some());
                assert!(
                    spec.menu != MenuPlace::None || !spec.bindings.is_empty(),
                    "UINV3: toolbar action {} has neither a menu path nor a binding",
                    spec.id
                );
            }
        }
        for spec in instrument_actions() {
            assert!(action_spec(spec.id).is_some());
            assert!(
                spec.menu != MenuPlace::None || !spec.bindings.is_empty(),
                "UINV3: instrument action {} has neither a menu path nor a binding",
                spec.id
            );
        }

        // Parameterized actions implement gpui::Action partial_eq cleanly.
        let r0 = OpenRecent { index: 0 };
        let r1 = OpenRecent { index: 1 };
        assert!(r0.partial_eq(&OpenRecent { index: 0 }));
        assert!(!r0.partial_eq(&r1));

        let p0 = PlacePart {
            symbol: SymbolId(0),
        };
        let p1 = PlacePart {
            symbol: SymbolId(1),
        };
        assert!(p0.partial_eq(&PlacePart {
            symbol: SymbolId(0)
        }));
        assert!(!p0.partial_eq(&p1));
    }
}
