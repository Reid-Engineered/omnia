# Omnia

Schematic capture and simulation, in the spirit of Multisim, rebuilt for today. The app is being written in Rust with GPUI.
This repository holds the Rust workspace (`crates/`: app shell, canvas, document model, symbols, theme and benchmarks) alongside the design work: FigCraft mockups, brand assets and the generators that build them.

![Workspace · Graphite](design/mockups/workspace-graphite.png)

## Contents

See the [project roadmap](ROADMAP.md) for the overall path from the technical foundation to a usable schematic and simulation application.

| Path | What it is |
| --- | --- |
| `crates/`, `xtask/` | The Rust workspace and its build tasks (`cargo xtask tokens`, `cargo xtask symbols`) |
| `AGENTS.md` | Standards and operating rules for contributors and agents (`CLAUDE.md` imports it) |
| `ROADMAP.md` | Product roadmap |
| `canvas-plan.md`, `canvas-roadmap.md` | Canvas technical plan and task list |
| `canvas-plan-prompt.md` | Original planning brief for the infinite-canvas workspace |
| `docs/` | Measurements, spike results and reference notes |
| `design/omnia.figcraft` | FigCraft document: Workspace, Home and First boot screens in the Graphite and Gray themes, the Mark component, and the logo and wordmark boards. Also the source of the theme tokens |
| `design/mockups/` | Screen exports (Workspace, Home, First boot in both themes) |
| `design/boards/` | Logo and wordmark board exports |
| `design/build-omnia.mjs` | Generates the screens (the FigCraft command script) |
| `design/build-logo-*.mjs`, `design/build-wordmark*.mjs`, `design/build-brand-svg.mjs` | Generate the logo boards, the wordmark boards and the brand SVGs |
| `design/symbols.mjs` | Loads the vendored schematic symbols and icons and places them as vector paths |
| `brand/` | The Omnia mark, the globe-O wordmark (Asimovian) and app icons as SVG |
| `vendor/` | Third-party symbols, icons and fonts; see `THIRD_PARTY.md` |

## Rebuilding the design files

```sh
cd design
npm install
node build-omnia.mjs          # writes capture4.json
figcraft-cli run --in <base>.figcraft --out omnia.figcraft capture4.json   # base = a file with the Theme tokens and the Mark component
node build-brand-svg.mjs      # rewrites ../brand/*.svg
```

## Design

- The themes are Graphite (dark) and Gray (light). Every colour comes from a design token.
- The UI is set in Inter, with values and designators in Source Code Pro. The wordmark is outlined Asimovian with the Lissajous globe as its O.
- Schematic symbols come from Chris Pikul's electronic-symbols (IEEE and IEC); UI icons come from Tabler Icons.
- The sidebar appears only on the Home screen. The workspace has a menu bar, toolbars, a full-width sheet and an instruments strip.
- Simulators: ngspice is bundled; RIS-C (a Rust SPICE engine) is available as a preview.

## Licences

Third-party assets keep their own licences (MIT, and SIL OFL 1.1 for Asimovian); see `THIRD_PARTY.md`.
