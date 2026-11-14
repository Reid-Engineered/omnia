// Omnia logo options: adds a "Logo options" page to omnia.figcraft.
import { writeFileSync } from "node:fs";
import { transformD } from "./symbols.mjs";

const SANS = "Inter", MONO = "Source Code Pro";
const DARK = "#0B0B0C", PANEL = "#161617", PAPER = "#EFEDE6", INK = "#111112", WHITE = "#F2F2F2";

// Marks drawn in a 150 box. Each part: { d, w (stroke in box units) } or { d, fill: true } or { d, w, cut: true } (drawn in the tile colour).
const ring = (cx, cy, r) => `M${cx - r} ${cy}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0`;
const disc = (cx, cy, r) => ring(cx, cy, r) + "Z";
const arc = (cx, cy, r, a0, a1) => { const p = (a) => [cx + r * Math.cos(a * Math.PI / 180), cy + r * Math.sin(a * Math.PI / 180)]; const [x0, y0] = p(a0), [x1, y1] = p(a1); return `M${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`; };
const liss = () => { let d = ""; for (let i = 0; i <= 240; i++) { const t = (i / 240) * 2 * Math.PI; const x = 75 + 52 * Math.sin(3 * t + Math.PI / 2), y = 75 + 44 * Math.sin(2 * t); d += `${i ? "L" : "M"}${x.toFixed(2)} ${y.toFixed(2)}`; } return d; };
const segmented = () => {
  const gaps = [-90, 30, 150], parts = [];
  for (let i = 0; i < 3; i++) parts.push({ d: arc(75, 75, 46, gaps[i] + 24, gaps[(i + 1) % 3] + (i === 2 ? 360 : 0) - 24), w: 12 });
  for (const g of gaps) parts.push({ d: disc(75 + 46 * Math.cos(g * Math.PI / 180), 75 + 46 * Math.sin(g * Math.PI / 180), 8), fill: true });
  return parts;
};

const lj = (A, B, ph, rx, ry, disc) => { let d = ""; for (let i = 0; i <= 900; i++) { const t = (i / 900) * 2 * Math.PI; let x = Math.sin(A * t + ph), y = Math.sin(B * t); if (disc) { const X = x * Math.sqrt(1 - (y * y / 2) * disc), Y = y * Math.sqrt(1 - (x * x / 2) * disc); x = X; y = Y; } d += `${i ? "L" : "M"}${(75 + rx * x).toFixed(2)} ${(75 + ry * y).toFixed(2)}`; } return d; };
const MARKS = [
  { name: "1 · Original", why: "3:2, 52 × 44. Open and calm: four crossings, big gaps, the clearest at 22 px.", parts: [{ d: liss(), w: 8 }] },
  { name: "1, rounder", why: "The same 3:2 figure pulled halfway to a circle (54 × 50). Keeps the openness of 1, loses most of the oval.", parts: [{ d: lj(3, 2, Math.PI / 2, 54, 50, 0.5), w: 8 }] },
  { name: "Halfway", why: "The 3:4 weave of 5 at the softer, half-round outline. More texture than 1, less of a ball than 5.", parts: [{ d: lj(3, 4, Math.PI / 2, 54, 50, 0.5), w: 7 }] },
  { name: "5, thinner", why: "5's round 3:4 globe with a lighter stroke so the gaps survive at small sizes.", parts: [{ d: lj(3, 4, Math.PI / 2, 56, 56, 1), w: 5.5 }] },
  { name: "5 · Round 3:4", why: "The full globe: round outline, dense mesh. Most distinctive large, fills in below 32 px.", parts: [{ d: lj(3, 4, Math.PI / 2, 56, 56, 1), w: 7 }] },
];

const cmds = [];
let n = 0;
const id = () => "L" + n++;
const ref = (a) => ({ $ref: a });
const push = (command, params, as) => { const c = { command, params }; if (as) c.as = as; cmds.push(c); return as; };
const frame = (parent, name, x, y, w, h, props = {}) => push("frame.create", { parent: parent && ref(parent), name, x, y, width: w, height: h, select: false, props: { fill: "none", clip: true, ...props } }, id());
const text = (parent, s, x, y, o = {}) => {
  const props = { fontFamily: o.mono ? MONO : SANS, fontStyle: o.style || "Regular", fontSize: o.size || 13, fill: o.color || WHITE };
  if (o.lh) props.lineHeight = o.lh;
  const p = { parent: ref(parent), text: s, x, y, select: false, props }; if (o.w) p.width = o.w;
  return push("text.create", p, id());
};
const drawMark = (parent, mk, x, y, size, fg, bg) => {
  const k = size / 150;
  for (const p of mk.parts) {
    const d = transformD(p.d, { x, y, size });
    const props = p.fill ? { fill: fg } : { fill: "none", stroke: p.cut ? bg : fg, strokeWeight: +(p.w * k).toFixed(2), strokeCap: "round", strokeJoin: "round" };
    push("vector.create", { parent: ref(parent), name: mk.name, path: d, x: 0, y: 0, select: false, props }, id());
  }
};

push("page.add", { name: "Lissajous 1 vs 5" });
const board = frame(null, "Omnia · Lissajous 1 vs 5", 0, 0, 2264, 480, { fill: DARK });
text(board, "Omnia", 32, 28, { size: 28, style: "SemiBold" });
text(board, "Between 1 and 5 · from open and calm to dense and round · dark, paper, 56 / 32 / 22 px icons, wordmark", 32, 66, { size: 14, color: "#A1A1A6" });
const CW = 424, CH = 340;
MARKS.forEach((mk, i) => {
  const cx = 32 + i * (CW + 16), cy = 108;
  const c = frame(board, `${i + 1} · ${mk.name}`, cx, cy, CW, CH, { fill: PANEL, cornerRadius: 14 });
  const dt = frame(c, "on dark", 12, 12, 194, 160, { fill: DARK, cornerRadius: 10 });
  drawMark(dt, mk, 37, 20, 120, WHITE, DARK);
  const lt = frame(c, "on paper", 218, 12, 194, 160, { fill: PAPER, cornerRadius: 10 });
  drawMark(lt, mk, 37, 20, 120, INK, PAPER);
  // app icons
  [[56, 14, 12], [32, 8, 184 - 98], [22, 6, 0]].forEach(([s, r], j) => {
    const x = [12, 78, 120][j], y = 184 + (56 - s) / 2;
    const ic = frame(c, `icon ${s}`, x, y, s, s, { fill: WHITE, cornerRadius: r });
    drawMark(ic, mk, s * 0.12, s * 0.12, s * 0.76, INK, WHITE);
  });
  // lockup
  drawMark(c, mk, 168, 190, 44, WHITE, PANEL);
  text(c, "Omnia", 220, 196, { size: 26, style: "SemiBold" });
  text(c, mk.name, 14, 255, { size: 16, style: "SemiBold" });
  text(c, mk.why, 14, 284, { size: 12.5, color: "#A1A1A6", w: CW - 28, lh: "150%" });
});

writeFileSync(new URL("./logo3.json", import.meta.url), JSON.stringify([cmds[0], { command: "command.batch", params: { label: "Lissajous 1 vs 5", commands: cmds.slice(1) } }]));
console.log("ok", cmds.length);
