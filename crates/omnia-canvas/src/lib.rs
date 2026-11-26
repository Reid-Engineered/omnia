//! Schematic canvas views, camera, and tools.

pub mod camera;
pub mod camera_anim;
pub mod input;
pub mod instrument_host;
pub mod keymap;
pub mod overlay;
pub mod selection;
pub mod sheet;
pub mod snap_px;
pub mod tool;

pub use camera::Camera;
pub use camera_anim::{
    CameraAnimation, FIT_FILL_FRACTION, VIEW_ANIM_DURATION, ViewAnimation, doc_bounds, ease_out,
    fit_area_camera, fit_rect_camera, fit_sheet_camera, selection_bounds, zoom_to_selection_camera,
};
pub use input::{CanvasEvent, CanvasStatus, Navigator, SheetCanvas};
pub use instrument_host::{
    DEFAULT_INSTRUMENT_SIZE, DEFAULT_TRACE_COLUMNS, InstrumentPaintScope, InstrumentRate,
    InstrumentStub, frame_stride,
};
pub use overlay::{CachedSheet, NetHighlight, OverlayEvent, OverlayPaintScope, OverlayView};
pub use selection::{RubberBand, Selection, SelectionState};
pub use sheet::SheetView;
pub use tool::{PlaceProvider, PlaceRequest};
