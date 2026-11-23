# Omnia project roadmap

Omnia is a desktop application for building electronic schematics and simulating circuits, written in Rust with GPUI. This roadmap describes how the whole product becomes usable. The [canvas roadmap](canvas-roadmap.md) provides the detailed implementation tasks beneath it, the [wiring roadmap](wiring-roadmap.md) covers wire behaviour, the [UI plan](ui-plan.md) and [roadmap](ui-roadmap.md) cover the application shell, and the [simulation plan](sim-plan.md) and [roadmap](sim-roadmap.md) cover ERC, engines (`ngspice` and `RIS-C`), and virtual instruments. The [agent-control plan](mcp-plan.md) and its [roadmap](mcp-roadmap.md) describe how agents and scripts drive Omnia through the same document commands as the editor; that surface serves the stages below and adds no product scope of its own.

This is an outcome-based roadmap, not a schedule. Stages can overlap, but a stage is complete only when its exit criteria are demonstrated. The first-release scope below is a proposed baseline; open product decisions are recorded at the end.

## First-release destination

A user can create a circuit from supported components, connect and edit it, save it, reopen it, run a supported simulation, and inspect useful results. Errors explain what needs fixing, and failures preserve the user's work.

The first release should prove this complete workflow on a small, documented set of circuits before expanding the component library or analysis types.

## Where the project stands

The repository contains Rust crates for the application shell (`omnia-app`), canvas (`omnia-canvas`), document model (`omnia-doc`), symbols (`omnia-symbols`), theme (`omnia-theme`), and benchmarks (`omnia-bench`). It also contains UI mockups, brand assets, generators, tests, and measurement reports.

The canvas has a detailed plan, and wiring planning is under way. The canvas task status and human performance gates remain authoritative; an existing crate or a passing local test does not mean a product milestone is complete. The README's design notes describe ngspice as bundled and RIS-C as a preview; those notes describe the mockups and do not establish a working integration.

## 1. Establish the technical foundation

**User outcome:** the application can display and navigate a representative schematic reliably.

Bring together the app shell, GPUI rendering, document model, symbols, theme, and benchmark harness. Establish performance and correctness evidence on the reference hardware.

**Ready when:** representative sheets render correctly, navigation meets the agreed budgets, document round trips preserve data, and the relevant canvas gates are accepted.

**Detailed work:** foundation, navigation, rendering, text, and document tasks in the [canvas roadmap](canvas-roadmap.md).

## 2. Deliver a usable schematic editor

**User outcome:** a user can build, change, save, and reopen a circuit through the desktop interface.

Connect component selection, placement, selection, movement, rotation, deletion, undo/redo, wiring, labels, and file actions. The application UI owns menus, toolbars, dialogs, and workspace controls; the canvas owns direct sheet interaction. Both operate on the same document model.

**Ready when:** a user can build an agreed example circuit without developer commands, edit it with predictable wire behavior, undo mistakes, and reopen it with its circuit data intact. File errors and unsaved changes have defined behavior.

**Detailed work:** editing and final integration in the [canvas roadmap](canvas-roadmap.md), wire behavior in the wiring roadmap once it is published. Product file flows and component browsing need planning beyond test-sheet serialization.

## 3. Complete the simulation workflow

**User outcome:** a user can simulate the circuit they drew and understand the result.

Define the connection between schematic components and simulation models, circuit validation, netlist generation, engine execution, cancellation, diagnostics, and result data. Add the UI for choosing an analysis, running it, and inspecting measurements or plots.

**Ready when:** an agreed set of reference circuits produces results within documented tolerances; invalid circuits give actionable diagnostics; a failed or cancelled run leaves the editor usable and the document intact.

**Detailed work:** ERC, netlist lowering, dual simulation engines (`ngspice` + `RIS-C`), analog models, live streaming, virtual instruments (`XSC1`, `XMM1`, `XBP1`, Results dock), and agent simulation tools in the [simulation plan](sim-plan.md) and [simulation roadmap](sim-roadmap.md). Owner decisions (2026-11-23): both engines in parallel from S1, full analog core (`OP`, `TRAN`, `AC`, `DC`), passives/sources/diode/BJT/MOSFET/op-amp models, and Oscilloscope + Multimeter + Bode Plotter + Results panel in v1.

## 4. Complete the desktop experience

**User outcome:** a new user can find their work, learn the basic workflow, and recover from ordinary mistakes.

Integrate Home and first-boot screens, project/file navigation, settings, keyboard shortcuts, instruments and results, help, and recovery behavior. Use the existing mockups as design inputs and verify their flows against the working application. Address accessibility throughout implementation.

**Ready when:** a new user can complete the first-release workflow from launch; loading, empty, error, and recovery states are covered; keyboard access and readability are checked.

**Planning needed:** application UI and file lifecycle plans. This work can proceed alongside editor and simulation development.

## 5. Prepare and validate a release

**User outcome:** users can install Omnia and complete its documented workflow on supported machines.

Choose supported platforms, package the app and simulation dependencies, validate real circuits, verify licenses, and provide setup instructions, examples, and a way to report issues. Define versioning, compatibility, and distribution/update behavior.

**Ready when:** a clean installation works on each supported platform, the reference circuits and file compatibility checks pass, required performance gates are accepted, and known limitations are documented.

**Planning needed:** a release checklist with concrete platform, packaging, and validation ownership.

## 6. Grow from a reliable baseline

Use feedback from the first complete workflow to prioritize broader component/model coverage, additional analyses, instruments, and productivity improvements. These are candidates, not committed scope. Give each selected addition its own outcome and acceptance criteria.

## Decisions still open

- Which operating systems are supported at launch?
- Does the initial file experience use standalone documents or a project container?
- What autosave, crash recovery, and file compatibility guarantees are required?

*(Answered 2026-11-23 in [sim-plan.md](sim-plan.md) `SD1`–`SD4`: both `ngspice` and `RIS-C` build in parallel from S1; v1 covers `OP`, `TRAN`, `AC`, `DC`, analog core passives/sources/semiconductors/op-amp, and Oscilloscope, Multimeter, Bode Plotter, and Results panel; finer defaults `SQ1`–`SQ10` await `HS1`.)*

## How to maintain this roadmap

Keep product outcomes here and technical task status in the implementation roadmaps. Link new UI, simulation, file lifecycle, and release plans here as they are written. Mark a stage complete with links to its acceptance evidence, not merely because its tasks have been implemented. Revise the proposed first-release scope when the open decisions are resolved; avoid attaching dates until scope and dependencies support them.
