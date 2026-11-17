# Third-party assets

## electronic-symbols

Schematic symbols on the canvas, in the parts toolbar and on the Home screen come from
[chris-pikul/electronic-symbols](https://github.com/chris-pikul/electronic-symbols)
(IEEE and common-style SVGs), vendored in `vendor/electronic-symbols/`.

MIT License, Copyright (c) 2022 Chris Pikul. Full text: `vendor/electronic-symbols/LICENSE`.

`design/symbols.mjs` reads the SVGs and turns them into FigCraft vectors (it rotates, mirrors and scales
the paths and sets the stroke weight); the drawings themselves are not modified.

## Asimovian

The Omnia wordmark is set in [Asimovian](https://github.com/carolinashort/asimovian) by Carolina Short,
vendored in `vendor/asimovian/` (fonts, OFL.txt, AUTHORS, README). SIL Open Font License 1.1.
`design/build-wordmark.mjs` outlines the glyphs from the TTF (via opentype.js), so the wordmark in the
FigCraft file is vector paths and the font does not need to be installed.

## Tabler Icons

Interface icons (toolbar, window controls, instruments strip, Home navigation, menus and dialogs)
come from [Tabler Icons](https://github.com/tabler/tabler-icons) v3.19.0 by Paweł Kuna. Only the
outline icons the mockup and shell use are vendored in `vendor/tabler-icons/outline/`. MIT License,
full text in `vendor/tabler-icons/LICENSE`.

Added for the application shell (`UD17`, v3.19.0 outline): `alert-triangle`, `check`,
`chevron-left`, `chevron-right`, `file`, `info-circle`, `keyboard`, `layout-bottombar`,
`layout-sidebar`, `restore`, `settings`, `table`, `trash`.

## Inter

UI and canvas label sans-serif font, from
[rsms/inter](https://github.com/rsms/inter) v4.1 (`Inter-4.1.zip`, `extras/ttf/`). Only the
static hinted weights the mockup uses (400 Regular, 500 Medium, 600 SemiBold) are vendored in
`vendor/fonts/inter/`.

SIL Open Font License 1.1, Copyright (c) 2016 The Inter Project Authors. Full text:
`vendor/fonts/inter/LICENSE.txt`.

## Source Code Pro

Monospace font for numeric readouts, tags and shortcuts, from
[adobe-fonts/source-code-pro](https://github.com/adobe-fonts/source-code-pro) release
`2.042R-u/1.062R-i/1.026R-vf` (`TTF-source-code-pro-2.042R-u_1.062R-i.zip`). Only the weights
the mockup uses (400 Regular, 500 Medium, 600 Semibold) are vendored in
`vendor/fonts/source-code-pro/`.

SIL Open Font License 1.1, Copyright © 2023 Adobe (Reserved Font Name 'Source'). Full text:
`vendor/fonts/source-code-pro/LICENSE.md`.
