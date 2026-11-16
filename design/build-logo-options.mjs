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

const MARKS = [
  { name: "Node ring", why: "An O sitting on a wire, as a net node. Current mark: quiet and reads as electronics without spelling a part.", parts: [{ d: ring(75, 75, 32) + " M12 75h31 M107 75h31", w: 12 }] },
  { name: "Heavy O", why: "Just the letter, very bold. Strongest at 16 px and the most neutral; relies on the wordmark for meaning.", parts: [{ d: ring(75, 75, 42), w: 24 }] },
  { name: "Junction", why: "A ring with one lead and a solder-dot junction where it leaves. One clear detail, still a clean O.", parts: [{ d: ring(66, 75, 40) + " M106 75h34", w: 12 }, { d: disc(106, 75, 12), fill: true }] },
  { name: "Ohm", why: "Omnia starts with the sound of ohm. An Ω built from one stroke with flat feet, like the unit symbol on a meter.", parts: [{ d: "M24 122h34v-13A46 46 0 1 1 92 109v13h34", w: 12 }] },
  { name: "AC source", why: "The sine-in-a-circle of an AC source symbol, so it reads as an O and as simulation at once.", parts: [{ d: ring(75, 75, 52), w: 11 }, { d: "M42 75c9-30 24-30 33 0s24 30 33 0", w: 11 }] },
  { name: "Lissajous", why: "A 3:2 Lissajous figure, the shape a scope draws in X/Y mode. Points at the instruments, busier at small sizes.", parts: [{ d: liss(), w: 8 }] },
  { name: "Three nets", why: "An O split into three arcs with node dots in the gaps: parts joined into one circuit, the 'all' in Omnia.", parts: segmented() },
  { name: "Cut disc", why: "A solid disc with a sine wave cut through it. Works as an app icon and holds up when filled with a brand colour.", parts: [{ d: disc(75, 75, 54), fill: true }, { d: "M14 75c12-34 30-34 45 0s33 34 45 0s21-26 34-14", w: 11, cut: true }] },
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

push("page.add", { name: "Logo options" });
const board = frame(null, "Omnia · logo options", 0, 0, 1824, 836, { fill: DARK });
text(board, "Omnia", 32, 28, { size: 28, style: "SemiBold" });
text(board, "Logo options · each shown on dark and paper, as app icons at 56 / 32 / 22 px, and with the wordmark", 32, 66, { size: 14, color: "#A1A1A6" });
const CW = 424, CH = 340;
MARKS.forEach((mk, i) => {
  const cx = 32 + (i % 4) * (CW + 16), cy = 108 + Math.floor(i / 4) * (CH + 16);
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
  text(c, `${i + 1}`, 14, 258, { mono: true, size: 12, color: "#6E6E73" });
  text(c, mk.name, 34, 255, { size: 16, style: "SemiBold" });
  text(c, mk.why, 14, 284, { size: 12.5, color: "#A1A1A6", w: CW - 28, lh: "150%" });
});

writeFileSync(new URL("./logo.json", import.meta.url), JSON.stringify([cmds[0], { command: "command.batch", params: { label: "Logo options", commands: cmds.slice(1) } }]));
console.log("ok", cmds.length);
