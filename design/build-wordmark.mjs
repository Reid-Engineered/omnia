// Omnia wordmark board set in Asimovian (OFL), outlined from the TTF so no font install is needed.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import opentype from "opentype.js";
import { transformD } from "./symbols.mjs";

const FONT = opentype.loadSync(fileURLToPath(new URL("../vendor/asimovian/fonts/ttf/Asimovian-Regular.ttf", import.meta.url)));
const DARK = "#0B0B0C", PANEL = "#161617", PAPER = "#EFEDE6", INK = "#111112", WHITE = "#F2F2F2", MUTED = "#A1A1A6";
const MARK = (() => { let d = ""; for (let i = 0; i <= 900; i++) { const t = (i / 900) * 2 * Math.PI; const x = Math.sin(3 * t + Math.PI / 2), y = Math.sin(4 * t); const X = x * Math.sqrt(1 - y * y / 2), Y = y * Math.sqrt(1 - x * x / 2); d += `${i ? "L" : "M"}${(75 + 56 * X).toFixed(2)} ${(75 + 56 * Y).toFixed(2)}`; } return d; })();

// text → outline path; tracking in em
function word(text, x, baseline, size, tracking = 0) {
  const glyphs = FONT.stringToGlyphs(text), k = size / FONT.unitsPerEm;
  let pen = x, d = "";
  glyphs.forEach((g, i) => {
    d += g.getPath(pen, baseline, size).toPathData(2) + " ";
    pen += g.advanceWidth * k;
    if (i < glyphs.length - 1) pen += FONT.getKerningValue(g, glyphs[i + 1]) * k + tracking * size;
  });
  return { d: d.trim(), width: pen - x };
}
const capH = (size) => (FONT.tables.os2.sCapHeight || FONT.unitsPerEm * 0.7) * size / FONT.unitsPerEm;

const cmds = [];
let n = 0;
const id = () => "W" + n++;
const ref = (a) => ({ $ref: a });
const push = (command, params, as) => { const c = { command, params }; if (as) c.as = as; cmds.push(c); return as; };
const frame = (parent, name, x, y, w, h, props = {}) => push("frame.create", { parent: parent && ref(parent), name, x, y, width: w, height: h, select: false, props: { fill: "none", clip: true, ...props } }, id());
const text = (parent, s, x, y, o = {}) => {
  const props = { fontFamily: o.mono ? "Source Code Pro" : "Inter", fontStyle: o.style || "Regular", fontSize: o.size || 13, fill: o.color || WHITE };
  const p = { parent: ref(parent), text: s, x, y, select: false, props }; if (o.w) { p.width = o.w; props.textAlign = o.align || "left"; }
  return push("text.create", p, id());
};
const mark = (parent, x, y, size, color, sw) => push("vector.create", { parent: ref(parent), name: "Omnia mark", path: transformD(MARK, { x, y, size }), x: 0, y: 0, select: false, props: { fill: "none", stroke: color, strokeWeight: sw ?? +(7 * size / 150).toFixed(2), strokeCap: "round", strokeJoin: "round" } }, id());
const wm = (parent, s, x, baseline, size, color, tracking = 0) => { const w = word(s, x, baseline, size, tracking); push("vector.create", { parent: ref(parent), name: "wordmark " + s, path: w.d, x: 0, y: 0, select: false, props: { fill: color } }, id()); return w.width; };
// horizontal lockup centred in a box; mark height ≈ 1.45 × cap height
const lockup = (parent, s, cx, cy, size, fg, tracking = 0) => {
  const ch = capH(size), ms = ch * 1.55, gap = ch * 0.5;
  const w = word(s, 0, 0, size, tracking).width, total = ms + gap + w, x0 = cx - total / 2;
  mark(parent, x0, cy - ms / 2, ms, fg);
  wm(parent, s, x0 + ms + gap, cy + ch / 2, size, fg, tracking);
};

push("page.add", { name: "Wordmark · Asimovian" });
const B = frame(null, "Omnia · wordmark in Asimovian", 0, 0, 1824, 1060, { fill: DARK });
text(B, "Omnia wordmark · Asimovian", 32, 28, { size: 28, style: "SemiBold" });
text(B, "Asimovian by Carolina Short (SIL OFL 1.1), outlined from the TTF · display only; the UI stays in Inter", 32, 66, { size: 14, color: MUTED });

const card = (name, x, y, w, h, bg) => frame(B, name, x, y, w, h, { fill: bg, cornerRadius: 14, stroke: "#FFFFFF1A", strokeWeight: 1 });
const label = (s, x, y) => text(B, s, x, y, { size: 12.5, color: MUTED });
// row 1: Omnia on dark / paper, OMNIA tracked, omnia
const W = 428, H = 240, Y1 = 108;
[["Omnia", 0, DARK, WHITE, "Omnia · on dark"], ["Omnia", 0, PAPER, INK, "Omnia · on paper"], ["OMNIA", 0.12, DARK, WHITE, "OMNIA · caps, tracked +12%"], ["omnia", 0.02, PAPER, INK, "omnia · lowercase"]].forEach(([s, tr, bg, fg, lab], i) => {
  const x = 32 + i * (W + 16);
  const c = card(lab, x, Y1, W, H, bg);
  c && lockup(c, s, W / 2, H / 2, 64, fg, tr);
  label(lab, x + 4, Y1 + H + 10);
});
// row 2: stacked, size ladder, in-app header
const Y2 = 392;
{
  const c = card("Stacked", 32, Y2, W, 300, DARK);
  mark(c, W / 2 - 56, 46, 112, WHITE);
  const w = word("Omnia", 0, 0, 52).width; wm(c, "Omnia", W / 2 - w / 2, 236, 52, WHITE);
  label("Stacked · splash and about box", 36, Y2 + 310);
}
{
  const c = card("Size ladder", 32 + W + 16, Y2, W * 2 + 16, 300, PANEL);
  let yy = 26;
  for (const s of [72, 40, 24, 16]) {
    const ch = capH(s), ms = ch * 1.55;
    mark(c, 28, yy, ms, WHITE, Math.max(0.9, +(7 * ms / 150).toFixed(2)));
    wm(c, "Omnia", 28 + ms + ch * 0.5, yy + ms / 2 + ch / 2, s, WHITE);
    text(c, `${s} px`, W * 2 + 16 - 90, yy + ms / 2 - 8, { mono: true, size: 11, color: MUTED, w: 60, align: "right" });
    yy += ms + (s > 30 ? 26 : 22);
  }
  label("Size ladder · the joined letterforms hold down to about 16 px", 32 + W + 20, Y2 + 310);
}
{
  const x = 32 + 3 * (W + 16), c = card("In the app", x, Y2, W, 300, "#1F1F21");
  // menu bar strip
  const mb = frame(c, "menu bar", 0, 0, W, 36, { fill: DARK });
  const lg = frame(mb, "logo", 12, 7, 22, 22, { fill: WHITE, cornerRadius: 6 }); mark(lg, 0, 0, 22, INK, 1.15);
  ["File", "Edit", "View", "Place", "Simulate"].reduce((xx, m) => { text(mb, m, xx, 10, { size: 13 }); return xx + m.length * 7.9 + 20; }, 48);
  // sidebar header
  const sb = frame(c, "sidebar header", 16, 52, W - 32, 120, { fill: PANEL, cornerRadius: 12 });
  const lg2 = frame(sb, "logo", 18, 18, 30, 30, { fill: WHITE, cornerRadius: 8 }); mark(lg2, 0, 0, 30, INK, 1.5);
  wm(sb, "Omnia", 60, 18 + 15 + capH(22) / 2, 22, WHITE);
  text(sb, "capture · simulate", 18, 64, { size: 11.5, color: MUTED });
  const nb = frame(sb, "New design", 18, 84, W - 68, 26, { fill: WHITE, cornerRadius: 7 }); text(nb, "New design", 10, 5, { size: 12, style: "SemiBold", color: INK });
  // window title
  text(c, "Window title and UI text stay in Inter:", 16, 190, { size: 12, color: MUTED });
  text(c, "wien-oscillator.omnia — Omnia", 16, 210, { size: 13 });
  text(c, "Tran: 0.093 s   grid 2.54   zoom 100%", 16, 236, { mono: true, size: 11.5, color: MUTED });
  label("In the app · menu bar logo, Home sidebar header", x + 4, Y2 + 310);
}
// row 3: alphabet strip to judge the voice
{
  const c = card("Alphabet", 32, 742, 1760, 280, PANEL);
  wm(c, "ABCDEFGHIJKLMNOPQRSTUVWXYZ", 28, 90, 50, WHITE);
  wm(c, "abcdefghijklmnopqrstuvwxyz 0123456789", 28, 170, 50, WHITE);
  wm(c, "Capture · Simulate · Measure", 28, 244, 40, MUTED);
}
// drop the placeholder lockup calls that drew at 0,0 with size 0
const clean = cmds;
writeFileSync(new URL("./wordmark.json", import.meta.url), JSON.stringify([clean[0], { command: "command.batch", params: { label: "Wordmark", commands: clean.slice(1) } }]));
console.log("ok", clean.length, "digits:", FONT.charToGlyphIndex("0") ? "yes" : "no", "capH", capH(100).toFixed(1));
