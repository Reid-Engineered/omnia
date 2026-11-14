# Third-party assets

## electronic-symbols

Schematic symbols on the canvas, in the parts toolbar and on the Home screen come from
[chris-pikul/electronic-symbols](https://github.com/chris-pikul/electronic-symbols)
(IEEE and common-style SVGs), vendored in `vendor/electronic-symbols/`.

MIT License, Copyright (c) 2022 Chris Pikul. Full text: `vendor/electronic-symbols/LICENSE`.

`symbols.mjs` reads the SVGs and turns them into FigCraft vectors (it rotates, mirrors and scales
the paths and sets the stroke weight); the drawings themselves are not modified.

## Asimovian

The Omnia wordmark is set in [Asimovian](https://github.com/carolinashort/asimovian) by Carolina Short,
vendored in `vendor/asimovian/` (fonts, OFL.txt, AUTHORS, README). SIL Open Font License 1.1.
`build-wordmark.mjs` outlines the glyphs from the TTF (via opentype.js), so the wordmark in the
FigCraft file is vector paths and the font does not need to be installed.

## Tabler Icons

Interface icons (toolbar, window controls, instruments strip, Home navigation) come from
[Tabler Icons](https://github.com/tabler/tabler-icons) v3.19.0 by Paweł Kuna. Only the outline icons the
mockup uses are vendored in `vendor/tabler-icons/outline/`. MIT License, full text in
`vendor/tabler-icons/LICENSE`.
