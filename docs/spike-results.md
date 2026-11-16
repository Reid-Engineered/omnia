# Spike results and rendering decision (T0.6, H1)

Decided 2026-11-15 with the owner at H1. **Outcome: Plan A** (stock GPUI: plain quads and atlas
sprites), with far-zoom simplification, direct glyph emission and a visible-label cap.
Plan B (cached image tiles) is the fallback if B4 misses at T2.7. No GPUI fork (Q7 not needed).

## Hardware

Reference machine (Q4, plan section 2.1): Orion, Lenovo ThinkPad T470, Intel Core i5-7300U
(2 cores / 4 threads), Intel HD Graphics 620 through Mesa ANV Vulkan 26.2.3 (GPUI's wgpu
renderer), Arch Linux, KDE Plasma on Wayland, native 1920x1080 at 60 Hz, 100% scaling (S1 also
at 125%), on AC. Ten interleaved repetitions per case, 600 measured frames each.
Raw data and method: `docs/measurements/T0.8.md`.

## Budget change (owner decision)

The brief's frame budgets (120 fps target, p99 < 8 ms at 1,000 parts and < 16 ms at 10,000)
cannot be shown on a 60 Hz panel and exceed what this machine's GPU can present on very large
sheets. The owner relaxed them:

| Budget | Was | Now |
| --- | --- | --- |
| B3, 1,000 parts | p99 frame < 8 ms | 60 fps: p99 frame < 16.7 ms, median present interval ≤ 1 refresh |
| B4, 10,000 parts | p99 frame < 16 ms | 30 fps: p99 frame < 33.3 ms, median present interval ≤ 2 refreshes |
| B5 | no frame > 16.7 ms (1k) | no frame > 33.3 ms (1k), > 50 ms (10k) |

## Thresholds against the reference machine

Median of ten run p99s unless stated. "Added" subtracts the bare-window baseline (0.17 ms).

| Threshold (retuned) | Limit | Measured | Result |
| --- | --- | --- | --- |
| T-quad, 19k quads (1,000-part load) | draw ≤ 12 ms | 10.6 ms | pass |
| T-quad, 65k quads with grid (10,000-part load) | draw ≤ 25 ms | 23.2 ms | pass |
| T-gpu, 19k load | median present interval ≤ 16.7 ms | 16.7 ms | pass |
| T-gpu, 65k with grid | median present interval ≤ 33.3 ms | 32.7 ms | pass, no margin |
| T-gpu, 70k quads, no grid | median present interval ≤ 33.3 ms | 67.1 ms | **fail** (GPU-bound) |
| T-sprite, 1k sprites | added ≤ 2 ms | 1.55 ms | pass |
| T-sprite quality and atlas cap | see plan | not measured on Linux (no `render_to_image`) | open |
| T-text, 12k glyphs, direct emission | added ≤ 4 ms | 10.8 ms | **fail** → label cap |
| T-text, 12k glyphs, `ShapedLine::paint` | added ≤ 4 ms | 16.3 ms | fail |
| T-path | ≤ 3 µs/path | 10.8–34 µs/path | fail; paths stay out of the sheet (D4) |
| T-replay, 60k | ≤ 40% | 49% | fail; hover repaints the sheet, fine while B3/B4 hold |
| T-present, 19k | dirty-to-present ≤ 16.7 ms | 13.2 ms | pass |

Outcome row: "T-quad passes at 19k, fails at 65k, or T-gpu fails at 65k" → Plan A plus a
simpler far zoom. The 65k-with-grid load only just holds 30 fps and quads alone at 70k do not,
so far zoom must cut primitives: merge collinear wire segments, drop the grid below K10's
pitch, draw symbols as LOD boxes. Re-measured at T2.7.

T-text: direct glyph emission costs about 5 µs per six-glyph label at p99 on this machine, so
4 ms fits about 700 labels. K9 now caps visible labels at 700 per frame, hiding the smallest
first.

## What this does not settle

- Sprite quality when rotated or mirrored, and atlas growth across zoom sweeps, were measured
  only on the Windows development machine (T0.4); Linux has no `render_to_image`. The atlas
  never evicts (T0.4: gigabytes over a full sweep), so zoom buckets must stay bounded (plan
  risks table).
- Present interval is a CPU-side timestamp after the swap-chain present (G13), not a
  compositor scan-out time.
- Windows numbers (ASUS TUF F16, Intel UHD; `docs/measurements/T0.4.md`, `T0.4-ref.md`) are
  recorded but gate nothing.

## The four "not settled by reading" items (plan section 4)

1. Atlas eviction: none (T0.4). Bound the sprite keys.
2. Mirrored sprite matrix: not culled; visual quality checked on Windows only (T0.4 S3).
3. `ShapedLine::paint` allocations: zero once warm (T0.4 and T0.8 S5), but one nested layer
   per call doubles the cost; use direct glyph emission.
4. `render_to_image` without a GPU: unsupported on Linux (T0.8); on Windows it needs an
   interactive session (T0.4). Golden images run on Windows.
