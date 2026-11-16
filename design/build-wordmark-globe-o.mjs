// "Globe as the O": the Omnia mark replaces the O of the Asimovian wordmark.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import opentype from "opentype.js";
import { transformD } from "./symbols.mjs";

const FONT = opentype.loadSync(fileURLToPath(new URL("../vendor/asimovian/fonts/ttf/Asimovian-Regular.ttf", import.meta.url)));
const DARK = "#0B0B0C", PANEL = "#161617", PAPER = "#EFEDE6", INK = "#111112", WHITE = "#F2F2F2", MUTED = "#A1A1A6";
const UPEM = FONT.unitsPerEm, STEM = 116; // vertical stem of I, in font units
const lissajous = (r) => { let d = ""; for (let i = 0; i <= 900; i++) { const t = (i / 900) * 2 * Math.PI; const x = Math.sin(3 * t + Math.PI / 2), y = Math.sin(4 * t); const X = x * Math.sqrt(1 - y * y / 2), Y = y * Math.sqrt(1 - x * x / 2); d += `${i ? "L" : "M"}${(75 + r * X).toFixed(2)} ${(75 + r * Y).toFixed(2)}`; } return d; };
const MESH = lissajous(56);
const CIRCLE = "M19 75a56 56 0 1 0 112 0a56 56 0 1 0-112 0";

const cmds = [];
let n = 0;
const id = () => "G" + n++;
const ref = (a) => ({ $ref: a });
const push = (command, params, as) => { const c = { command, params }; if (as) c.as = as; cmds.push(c); return as; };
const frame = (parent, name, x, y, w, h, props = {}) => push("frame.create", { parent: parent && ref(parent), name, x, y, width: w, height: h, select: false, props: { fill: "none", clip: true, ...props } }, id());
const text = (parent, s, x, y, o = {}) => {
  const props = { fontFamily: o.mono ? "Source Code Pro" : "Inter", fontStyle: o.style || "Regular", fontSize: o.size || 13, fill: o.color || WHITE };
  const p = { parent: ref(parent), text: s, x, y, select: false, props }; if (o.w) { p.width = o.w; props.textAlign = o.align || "left"; }
  return push("text.create", p, id());
};
const vecP = (parent, name, d, props) => push("vector.create", { parent: ref(parent), name, path: d, x: 0, y: 0, select: false, props }, id());

// Variant style: mesh = mesh stroke as a fraction of the stem; ring = draw an outer ring at full stem; over = optical overshoot
// Lays out text whose first letter is O/o with the globe in its place. Returns width.
function globeWord(parent, txt, x, baseline, size, color, st, tracking = 0, dry = false) {
  const k = size / UPEM, first = txt[0], rest = txt.slice(1);
  const g = FONT.charToGlyph(first), bb = g.getBoundingBox();
  const top = baseline - bb.y2 * k, bot = baseline - bb.y1 * k;
  const H = (bot - top) * (st.over || 1), stem = STEM * k;
  const sb = 50 * k;                                 // keep the O's side bearings
  const D = H;                                       // round: diameter = height
  const cx = x + sb + D / 2, cy = (top + bot) / 2 + 0.015 * size; // sits slightly low, optically
  if (!dry) {
    const meshW = stem * st.mesh;
    const outerW = st.ring ? stem : meshW;
    const S = ((D - outerW) * 150) / 112;            // box so the outer edge of the stroke meets D
    const box = { x: cx - S / 2, y: cy - S / 2, size: S };
    if (st.ring) vecP(parent, "globe ring", transformD(CIRCLE, box), { fill: "none", stroke: color, strokeWeight: +outerW.toFixed(2) });
    vecP(parent, "globe mesh", transformD(MESH, box), { fill: "none", stroke: color, strokeWeight: +meshW.toFixed(2), strokeJoin: "round", strokeCap: "round" });
  }
  let pen = x + sb + D + sb - 0.03 * size + tracking * size; // optical spacing
  const glyphs = FONT.stringToGlyphs(rest);
  let d = "";
  glyphs.forEach((gl, i) => {
    d += gl.getPath(pen, baseline, size).toPathData(2) + " ";
    pen += gl.advanceWidth * k;
    if (i < glyphs.length - 1) pen += FONT.getKerningValue(gl, glyphs[i + 1]) * k + tracking * size;
  });
  if (!dry) vecP(parent, "letters " + rest, d.trim(), { fill: color });
  return pen - x;
}
const centred = (parent, txt, cw, ch, size, color, st, tracking = 0) => {
  const w = globeWord(null, txt, 0, 0, size, color, st, tracking, true);
  const capPx = (txt[0] === "O" ? 700 : 535) * size / UPEM;
  globeWord(parent, txt, cw / 2 - w / 2, ch / 2 + capPx / 2, size, color, st, tracking);
};

const STYLES = [
  { key: "A", name: "Ring + medium mesh", why: "Ring at the stem weight, weave at about 60% of it. The boldest version that still shows the pattern.", st: { mesh: 0.62, ring: true, over: 1.03 } },
  { key: "B", name: "Ring + fine mesh", why: "Outer ring at the stem weight so the O matches the letters; the weave inside is lighter. Best balance.", st: { mesh: 0.42, ring: true, over: 1.03 } },
  { key: "C", name: "Fine mesh", why: "The globe at about half the stem weight, no ring. Most like the icon; lighter than the letters beside it.", st: { mesh: 0.5, over: 1.04 } },
  { key: "D", name: "Caps · ring + mesh", why: "OMNIA in capitals, tracked +10%, with the ring-and-fine-mesh O.", st: { mesh: 0.42, ring: true, over: 1.03 }, txt: "OMNIA", tr: 0.1 },
];

push("page.add", { name: "Wordmark · globe O" });
const B = frame(null, "Omnia · globe as the O", 0, 0, 1824, 1000, { fill: DARK });
text(B, "Omnia · the globe as the O", 32, 28, { size: 28, style: "SemiBold" });
text(B, "Asimovian letters with the Lissajous globe in place of the O, sized to the font's O and matched to its stem weight", 32, 66, { size: 14, color: MUTED });
const card = (name, x, y, w, h, bg) => frame(B, name, x, y, w, h, { fill: bg, cornerRadius: 14, stroke: "#FFFFFF1A", strokeWeight: 1 });
const W = 428, H = 200;
STYLES.forEach((v, i) => {
  const x = 32 + i * (W + 16);
  const c1 = card(`${v.key} · ${v.name} · dark`, x, 108, W, H, PANEL);
  centred(c1, v.txt || "Omnia", W, H, 76, WHITE, v.st, v.tr || 0);
  const c2 = card(`${v.key} · ${v.name} · paper`, x, 108 + H + 12, W, H, PAPER);
  centred(c2, v.txt || "Omnia", W, H, 76, INK, v.st, v.tr || 0);
  text(B, `${v.key}`, x + 4, 108 + 2 * H + 26, { mono: true, size: 12, color: "#6E6E73" });
  text(B, v.name, x + 22, 108 + 2 * H + 23, { size: 15, style: "SemiBold" });
  text(B, v.why, x + 4, 108 + 2 * H + 48, { size: 12.5, color: MUTED, w: W - 8 });
});
// ladder for B and C
const Y3 = 640;
[["B", STYLES[1]], ["C", STYLES[2]]].forEach(([k, v], j) => {
  const x = 32 + j * (W + 16), c = card(`${k} · size ladder`, x, Y3, W, 300, PANEL);
  let base = 74;
  for (const s of [64, 36, 22, 16]) {
    globeWord(c, "Omnia", 24, base, s, WHITE, v.st);
    text(c, `${s} px`, W - 84, base - s * 0.45, { mono: true, size: 11, color: MUTED, w: 60, align: "right" });
    base += s * 0.7 + (s > 30 ? 42 : 30);
  }
  text(B, `${k} · size ladder`, x + 4, Y3 + 310, { size: 12.5, color: MUTED });
});
// lowercase + stacked comparison
{
  const x = 32 + 2 * (W + 16), c = card("lowercase · omnia", x, Y3, W, 300, PAPER);
  centred(c, "omnia", W, 300, 84, INK, STYLES[2].st, 0.02);
  text(B, "omnia · lowercase, globe at x-height (C)", x + 4, Y3 + 310, { size: 12.5, color: MUTED });
}
{
  const x = 32 + 3 * (W + 16), c = card("in the app", x, Y3, W, 300, "#1F1F21");
  const sb = frame(c, "sidebar header", 16, 16, W - 32, 96, { fill: PANEL, cornerRadius: 12 });
  globeWord(sb, "Omnia", 18, 50, 30, WHITE, STYLES[1].st);
  text(sb, "capture · simulate", 20, 64, { size: 11.5, color: MUTED });
  const sp = frame(c, "splash", 16, 124, W - 32, 160, { fill: DARK, cornerRadius: 12 });
  centred(sp, "Omnia", W - 32, 130, 52, WHITE, STYLES[1].st);
  text(sp, "Loading part libraries…", 0, 124, { size: 12, color: MUTED, w: W - 32, align: "center" });
  text(B, "In the app · Home sidebar header and splash (B)", x + 4, Y3 + 310, { size: 12.5, color: MUTED });
}
writeFileSync(new URL("./globe-o.json", import.meta.url), JSON.stringify([cmds[0], { command: "command.batch", params: { label: "Globe O", commands: cmds.slice(1) } }]));
console.log("ok", cmds.length);
