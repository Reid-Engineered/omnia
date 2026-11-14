// Generates a FigCraft `run` script for Omnia, a standalone schematic capture and
// simulation app mockup (workspace + home) built on the Spectare design system (tokens + Mark component).
import { writeFileSync } from "node:fs";
import { symbolPaths, pinAt, transformD, tablerD } from "./symbols.mjs";
import { fileURLToPath } from "node:url";
import opentype from "opentype.js";
// Wordmark: Asimovian (OFL) with the globe as the O, style C (fine mesh, no ring)
const WM_FONT = opentype.loadSync(fileURLToPath(new URL("./vendor/asimovian/fonts/ttf/Asimovian-Regular.ttf", import.meta.url)));
function wordmarkPaths(x, baseline, size) {
  const k = size / WM_FONT.unitsPerEm, bb = WM_FONT.charToGlyph("O").getBoundingBox();
  const top = baseline - bb.y2 * k, bot = baseline - bb.y1 * k, D = (bot - top) * 1.04, sw = 116 * k * 0.5, sb = 50 * k;
  const S = ((D - sw) * 150) / 112, cx = x + sb + D / 2, cy = (top + bot) / 2 + 0.015 * size; // sits slightly low, optically
  const globe = transformD(OMNIA_MARK, { x: cx - S / 2, y: cy - S / 2, size: S });
  let pen = x + sb + D + sb - 0.03 * size, d = ""; // optical spacing: a round O sits slightly closer
  const gl = WM_FONT.stringToGlyphs("mnia");
  gl.forEach((g, i) => { d += g.getPath(pen, baseline, size).toPathData(2) + " "; pen += g.advanceWidth * k + (i < gl.length - 1 ? WM_FONT.getKerningValue(g, gl[i + 1]) * k : 0); });
  return { globe, sw: +sw.toFixed(2), letters: d.trim(), width: pen - x };
}
// Omnia mark: round 3:4 Lissajous (logo option 5), drawn in a 150 box
const OMNIA_MARK = (() => { let d = ""; for (let i = 0; i <= 900; i++) { const t = (i / 900) * 2 * Math.PI; let x = Math.sin(3 * t + Math.PI / 2), y = Math.sin(4 * t); const X = x * Math.sqrt(1 - y * y / 2), Y = y * Math.sqrt(1 - x * x / 2); d += `${i ? "L" : "M"}${(75 + 56 * X).toFixed(2)} ${(75 + 56 * Y).toFixed(2)}`; } return d; })();

const MONO = "Source Code Pro", SANS = "Inter";
const INK = "#1D1D1F", INK2 = "#6B6B70", SEL = "#2F6FEB", SEL_HALO = "#9DBDFF";
const V = (n) => ({ $var: n });

function build(theme, frameX, screen, frameY) {
  const cmds = [];
  let n = 0;
  const id = () => `${screen}${theme[0]}${n++}`;
  const ref = (a) => ({ $ref: a });
  const push = (command, params, as) => { const c = { command, params }; if (as) c.as = as; cmds.push(c); return as; };

  const frame = (parent, name, x, y, w, h, props = {}) =>
    push("frame.create", { parent: parent && ref(parent), name, x, y, width: w, height: h, select: false, props: { fill: "none", clip: true, ...props } }, id());
  const rect = (parent, name, x, y, w, h, props = {}) =>
    push("node.create", { type: "rectangle", parent: ref(parent), name, x, y, width: w, height: h, select: false, props }, id());
  const ell = (parent, name, x, y, w, h, props = {}) =>
    push("node.create", { type: "ellipse", parent: ref(parent), name, x, y, width: w, height: h, select: false, props }, id());
  const vec = (parent, name, path, x, y, props = {}) =>
    push("vector.create", { parent: ref(parent), name, path, x, y, select: false, props: { fill: "none", ...props } }, id());
  const text = (parent, s, x, y, o = {}) => {
    const { mono, size = 13, style = "Regular", color = "text/primary", w, align, lh, name } = o;
    const props = { fontFamily: mono ? MONO : SANS, fontStyle: style, fontSize: size, fill: color.startsWith("#") ? color : V(color) };
    if (align) props.textAlign = align;
    if (lh) props.lineHeight = lh;
    const p = { parent: ref(parent), text: s, x, y, select: false, props };
    if (name) p.name = name;
    if (w) p.width = w;
    return push("text.create", p, id());
  };
  const rtext = (parent, s, right, y, o = {}) => text(parent, s, right - 300, y, { ...o, w: 300, align: "right" });
  const mark = (parent, kind, x, y) => {
    const m = push("instance.create", { component: "6732", parent: ref(parent), x, y }, id());
    push("instance.setProperties", { ids: [ref(m)], properties: { Kind: kind } });
    return m;
  };
  const pill = (parent, s, x, y) => { // count pill
    const f = frame(parent, "Count pill", x, y, 22, 18, { fill: V("bg/raised"), cornerRadius: 9 });
    text(f, s, 0, 2, { mono: true, size: 11, color: "text/secondary", w: 22, align: "center" });
    return f;
  };
  const inkTag = (parent, s, x, y, o = {}) => { // dark label pill on the paper
    const w = Math.round(s.length * 6.6 + 14);
    const f = frame(parent, "tag " + s, x, y, w, 20, { fill: o.fill || "#111112", cornerRadius: 4, ...(o.props || {}) });
    text(f, s, 0, 3, { mono: true, size: 10.5, color: o.color || "#F2F2F2", w, align: "center" });
    return f;
  };
  // "ti:<name>" draws a Tabler icon (MIT) at x,y with pixel size sz; anything else is raw path data
  const icon = (parent, name, path, x, y, color = "text/secondary", sw = 1.5, sz = 18) =>
    vec(parent, name, path.startsWith("ti:") ? tablerD(path.slice(3), x, y, sz) : path, path.startsWith("ti:") ? 0 : x, path.startsWith("ti:") ? 0 : y, { stroke: color.startsWith("#") ? color : V(color), strokeWeight: sw, strokeCap: "round", strokeJoin: "round" });

  const root = push("frame.create", { x: frameX, y: frameY, width: 1440, height: 900, name: `${{ work: "Workspace", home: "Home", first: "First boot" }[screen]} · ${theme}`, select: false, props: { fill: V("bg/backdrop"), clip: true } }, id());
  push("variable.setMode", { ids: [ref(root)], collection: "Theme", mode: theme });
  // place a Pikul symbol (MIT) as vectors; color is a hex string or a variable name
  const placeSym = (parent, name, box, color, sw, label) => {
    const paint = color.startsWith("#") ? color : V(color);
    for (const { d, filled } of symbolPaths(name, box))
      vec(parent, label || name, d, 0, 0, filled ? { fill: paint } : { stroke: paint, strokeWeight: sw, strokeJoin: "round", strokeCap: "round" });
  };
  const btn = (parent, s, x, y, w, primary, key, h = 30) => {
    const b = frame(parent, "Button " + s, x, y, w, h, { fill: primary ? V("button/primary") : V("bg/raised"), cornerRadius: 8 });
    text(b, s, key ? 12 : 0, (h - 16) / 2, { size: 12, style: "SemiBold", color: primary ? "button/on-primary" : "text/primary", ...(key ? {} : { w, align: "center" }) });
    if (key) text(b, key, w - 26, (h - 14) / 2, { mono: true, size: 10, color: primary ? "button/on-primary" : "text/muted" });
    return b;
  };
  const ICON = {
    new: "ti:file-plus", open: "ti:folder-open", save: "ti:device-floppy", print: "ti:printer",
    undo: "ti:arrow-back-up", redo: "ti:arrow-forward-up", cut: "ti:scissors", copy: "ti:copy", paste: "ti:clipboard",
    zin: "ti:zoom-in", zout: "ti:zoom-out", zarea: "ti:zoom-in-area", fit: "ti:arrows-maximize", full: "ti:maximize",
    mcu: "ti:cpu",
  };
  const iconBtn = (parent, name, d, x, y, o = {}) => {
    const b = frame(parent, "tool/" + name, x, y, 30, 30, { fill: o.active ? V("bg/raised") : "none", cornerRadius: 7, clip: false });
    icon(b, "icon", d, 6, 6, o.active ? "text/primary" : "text/secondary", 1.35);
    return b;
  };
  const group = (parent, name, x, y, names) => {
    const g = frame(parent, name, x, y, 4 + names.length * 32, 34, { fill: V("bg/inset"), cornerRadius: 9 });
    names.forEach((k, i) => iconBtn(g, k, ICON[k], 2 + i * 32, 2));
    return 4 + names.length * 32;
  };
  const menuBar = (root) => {
    const mb = frame(root, "Menu bar", 0, 0, 1440, 36, { fill: V("bg/backdrop") });
    const lg = frame(mb, "Logo", 12, 7, 22, 22, { fill: "#F2F2F2", cornerRadius: 6 });
    vec(lg, "Omnia mark", transformD(OMNIA_MARK, { x: 0, y: 0, size: 22 }), 0, 0, { stroke: "#111112", strokeWeight: 1.15, strokeCap: "round", strokeJoin: "round" });
    let mx = 48;
    for (const m of ["File", "Edit", "View", "Place", "Simulate", "Tools", "Reports", "Window", "Help"]) { text(mb, m, mx, 10, { size: 13 }); mx += m.length * 7.9 + 20; }
    return mb;
  };
  const winCtl = (mb, title) => {
    text(mb, title, 520, 10, { size: 12.5, color: "text/secondary", w: 400, align: "center" });
    icon(mb, "min", "ti:minus", 1348, 10, "text/secondary", 1.3, 16);
    icon(mb, "max", "ti:square", 1378, 10, "text/secondary", 1.3, 16);
    icon(mb, "close", "ti:x", 1408, 10, "text/secondary", 1.3, 16);
  };
  const inst = [
    ["multimeter", "ti:gauge"], ["function generator", "ti:wave-sine"], ["wattmeter", "ti:bolt"], ["oscilloscope", "ti:device-desktop-analytics"],
    ["4-channel scope", "ti:chart-line"], ["Bode plotter", "ti:chart-ppf"], ["frequency counter", "ti:stopwatch"], ["word generator", "ti:binary"],
    ["logic analyzer", "ti:wave-square"], ["logic converter", "ti:arrows-exchange"], ["IV analyzer", "ti:chart-dots"], ["distortion analyzer", "ti:wave-saw-tool"],
    ["spectrum analyzer", "ti:chart-histogram"], ["network analyzer", "ti:network"],
  ];

  // =====================================================================
  if (screen === "work") {
    const mb = menuBar(root);
    winCtl(mb, "wien-oscillator.omnia — Omnia");
    // ---------- toolbar ----------
    const tb = frame(root, "Toolbar", 0, 36, 1440, 50, { fill: V("bg/panel") });
    let tx = 10;
    tx += group(tb, "File", tx, 8, ["new", "open", "save", "print"]) + 8;
    tx += group(tb, "Edit", tx, 8, ["undo", "redo", "cut", "copy", "paste"]) + 8;
    tx += group(tb, "View", tx, 8, ["zin", "zout", "zarea", "fit", "full"]) + 14;
    rect(tb, "divider", tx - 8, 14, 1, 22, { fill: V("rule") });
    const parts = frame(tb, "Place parts", tx, 8, 4 + 12 * 32, 34, { fill: V("bg/inset"), cornerRadius: 9 });
    [["sources", "Source-COM-DC"], ["basic", "Resistor-IEEE-Standard"], ["diodes", "Diode-COM-Standard"], ["transistors", "Transistor-COM-BJT-NPN"], ["analog", "IC-COM-OpAmp"], ["logic", "IC-COM-Logic-AND"], ["mcu", null], ["indicators", "Diode-COM-LED"], ["power", "Source-COM-Battery-Single"], ["misc", "Miscellaneous-COM-Crystal_Oscillator"], ["electromech", "Relay-IEEE-SPST-NO"], ["switches", "Switch-COM-SPST"]].forEach(([k, symName], i) => {
      const active = k === "analog";
      if (!symName) return iconBtn(parts, k, ICON.mcu, 2 + i * 32, 2);
      const b = frame(parts, "tool/" + k, 2 + i * 32, 2, 30, 30, { fill: active ? V("bg/raised") : "none", cornerRadius: 7, clip: false });
      placeSym(b, symName, { x: 4, y: 4, size: 22 }, active ? "text/primary" : "text/secondary", 1.35, "symbol");
    });
    tx += 4 + 12 * 32 + 8;
    const iu = frame(tb, "In use list", tx, 8, 158, 34, { fill: V("bg/inset"), cornerRadius: 9, stroke: V("rule"), strokeWeight: 1 });
    text(iu, "In use · 11 parts", 12, 9, { size: 12 });
    icon(iu, "chev", "ti:chevron-down", 134, 9, "text/secondary", 1.4, 16);
    // right: analysis + run switch
    const an = frame(tb, "Analysis", 1086, 8, 118, 34, { fill: V("bg/inset"), cornerRadius: 9, stroke: V("rule"), strokeWeight: 1 });
    text(an, "Transient", 12, 9, { size: 12, style: "SemiBold" });
    icon(an, "chev", "ti:chevron-down", 94, 9, "text/secondary", 1.4, 16);
    const rc = frame(tb, "Sim controls", 1212, 8, 136, 34, { fill: V("bg/inset"), cornerRadius: 9 });
    const run = frame(rc, "run (active)", 3, 3, 60, 28, { fill: V("button/primary"), cornerRadius: 7 });
    icon(run, "play", "ti:player-play", 8, 7, "button/on-primary", 1.5, 14);
    text(run, "Run", 26, 7, { size: 12, style: "SemiBold", color: "button/on-primary" });
    icon(rc, "pause", "ti:player-pause", 72, 9, "text/secondary", 1.5, 16);
    icon(rc, "stop", "ti:player-stop", 104, 9, "text/secondary", 1.5, 16);
    const live = frame(tb, "live", 1356, 13, 74, 24, { fill: "none", cornerRadius: 12, stroke: V("rule"), strokeWeight: 1 });
    ell(live, "dot", 10, 9, 6, 6, { fill: V("accent/mint") });
    text(live, "0.093 s", 21, 5, { mono: true, size: 10.5, color: "text/secondary" });

    // ---------- canvas ----------
    const CW = 1366, CH = 718;
    const C = frame(root, "Canvas", 8, 92, CW, CH, { fill: V("stage/paper"), cornerRadius: 10 });
    let dots = "";
    for (let gx = 10; gx < CW; gx += 20) for (let gy = 10; gy < CH; gy += 20) dots += `M${gx} ${gy}h1.2v1.2h-1.2z`;
    push("vector.create", { parent: ref(C), name: "grid", path: dots, x: 0, y: 0, select: false, props: { fill: "#00000022" } }, id());
    const A_COL = SEL, B_COL = "#7A5AF8";
    const path = (pts) => pts.map(([x, y], i) => `${i ? "L" : "M"}${x} ${y}`).join(" ");
    const wire = (name, pts, c = INK, w = 1.8) => vec(C, name, path(pts), 0, 0, { stroke: c, strokeWeight: w, strokeJoin: "round" });
    const dv = (name, d, o = {}) => vec(C, name, d, 0, 0, { stroke: o.c || INK, strokeWeight: o.w || 1.8, strokeJoin: "round", strokeCap: "round", ...(o.dash ? { dashes: o.dash } : {}) });
    const dot = (x, y, c = INK) => ell(C, "junction", x - 4, y - 4, 8, 8, { fill: c });
    const lab = (r, v, x, y, o = {}) => { text(C, r, x, y, { size: 17, style: "SemiBold", color: INK }); if (v) text(C, v, x, y + 22, { size: 17, style: "SemiBold", color: INK }); };
    const zig = (x, y, len, vert) => { // resistor body, 60 long
      let d = `M${x} ${y}`; const s = len / 8;
      const pts = [[s, 0], [s / 2, -9], [s, 18], [s, -18], [s, 18], [s, -18], [s / 2, 9], [s, 0]];
      for (const [a, b] of pts) d += vert ? ` l${b} ${a}` : ` l${a} ${b}`;
      return d;
    };
    const cap = (x, y, vert) => vert ? `M${x - 16} ${y} h32 M${x - 16} ${y + 10} h32` : `M${x} ${y - 16} v32 M${x + 10} ${y - 16} v32`;
    const gnd = (x, y) => dv("GND", `M${x} ${y} v14 M${x - 14} ${y + 14} h28 M${x - 9} ${y + 20} h18 M${x - 4} ${y + 26} h8`);

    // nodes — symbols are Chris Pikul's electronic-symbols (MIT), IEEE style
    const sym = (name, box, label) => placeSym(C, name, box, INK, 2, label);
    const OUT = [1270, 390], NP = [120, 360], NM = [1000, 420];
    // op-amp U1 (box 180: + in at y50, − in at y100, out at x150, supplies top/bottom)
    const U1 = { x: 1030, y: 300, size: 180 };
    sym("IC-COM-OpAmp", U1, "U1");
    text(C, "3", 1036, 336, { size: 13, color: SEL, style: "Medium" }); text(C, "2", 1036, 396, { size: 13, color: SEL, style: "Medium" });
    lab("U1", "TL072", 1194, 292);
    inkTag(C, "+12V", 1128, 298); inkTag(C, "−12V", 1128, 462);
    wire("out", [pinAt(U1, 150, 75), OUT]); dot(...OUT);
    // feedback ladder: D1 / R4 / D2 between (1040,120) and (1220,120)
    wire("fb left", [NM, [1000, 120], [1040, 120]]);
    wire("fb right", [[1220, 120], [1270, 120], OUT]);
    dv("ladder", "M1040 70 V170 M1220 70 V170");
    dot(1040, 120); dot(1220, 120);
    sym("Diode-COM-Standard", { x: 1090, y: 30, size: 80 }, "D1");
    sym("Resistor-IEEE-Standard", { x: 1090, y: 80, size: 80 }, "R4");
    sym("Diode-COM-Standard", { x: 1090, y: 130, size: 80, rot: 180 }, "D2");
    dv("ladder taps", "M1040 70 H1090 M1170 70 H1220 M1040 120 H1090 M1170 120 H1220 M1040 170 H1090 M1170 170 H1220");
    lab("D1", "1N4148", 1090, 4); lab("D2", "1N4148", 1090, 194);
    text(C, "R4  22 kΩ", 1102, 133, { size: 12, color: INK2, style: "Medium" });
    wire("to −", [NM, pinAt(U1, 0, 100)]); dot(...NM);
    // R3 potentiometer to ground, interactive (rotated so the ends are top/bottom, wiper from the left)
    const R3 = { x: 960, y: 520, size: 80, rot: 90 };
    wire("R3 top", [NM, pinAt(R3, 0, 75)]);
    sym("Resistor-IEEE-Potentiometer", R3, "R3");
    const G3 = { x: 976, y: 600, size: 48 };
    sym("Ground-COM-General", G3, "GND");
    rect(C, "R3 interactive", 944, 506, 104, 96, { fill: "none", stroke: INK2, strokeWeight: 1.4, dashes: [6, 4], cornerRadius: 4 });
    lab("R3", null, 1078, 500); text(C, "Key = A", 1078, 524, { size: 16, style: "SemiBold", color: INK }); text(C, "50%", 1078, 548, { size: 14, style: "Medium", color: INK2 });
    // positive network
    wire("to +", [NP, pinAt(U1, 0, 50)]); dot(...NP);
    const R1 = { x: 740, y: 620, size: 80 }, C1 = { x: 480, y: 620, size: 80 };
    wire("out down", [OUT, [1270, 660], pinAt(R1, 150, 75)]);
    sym("Resistor-IEEE-Standard", R1, "R1");
    wire("R1-C1", [pinAt(R1, 0, 75), pinAt(C1, 150, 75)]);
    sym("Capacitor-IEEE-NonPolarized", C1, "C1");
    wire("C1-NP", [pinAt(C1, 0, 75), [120, 660], NP]);
    lab("R1", "10 kΩ", 750, 584); lab("C1", "22 nF", 490, 584);
    const R2 = { x: 200, y: 400, size: 80, rot: 90 }, C2 = { x: 290, y: 416, size: 80, rot: 90 };
    wire("R2 top", [[240, 360], pinAt(R2, 0, 75)]); dot(240, 360);
    sym("Resistor-IEEE-Standard", R2, "R2"); wire("R2 bot", [pinAt(R2, 150, 75), [240, 560]]);
    wire("C2 top", [[330, 360], pinAt(C2, 0, 75)]); dot(330, 360);
    sym("Capacitor-IEEE-NonPolarized", C2, "C2"); wire("C2 bot", [pinAt(C2, 150, 75), [330, 560]]);
    wire("gnd bus", [[240, 560], [330, 560]]); dot(285, 560);
    sym("Ground-COM-General", { x: 261, y: 560, size: 48 }, "GND");
    lab("R2", "10 kΩ", 150, 424); lab("C2", "22 nF", 352, 456);
    text(C, "N+", 128, 334, { size: 13, color: INK2, style: "Medium" });
    // scope connections + instrument symbol
    wire("XSC1 A", [[1270, 560], [1230, 560], [1230, 590]], A_COL, 1.8); dot(1270, 560, A_COL);
    wire("XSC1 B", [[1000, 486], [1170, 486], [1170, 590]], B_COL, 1.8); dot(1000, 486, B_COL);
    rect(C, "XSC1", 1144, 590, 108, 46, { fill: "#FBFAF6", stroke: INK, strokeWeight: 1.6, cornerRadius: 6 });
    vec(C, "XSC1 screen", "M1182 602 h32 v22 h-32 z M1185 616 l6 -7 l6 10 l6 -8 l6 4", 0, 0, { stroke: INK2, strokeWeight: 1.2 });
    text(C, "B", 1165, 593, { size: 11, color: B_COL, style: "SemiBold" }); text(C, "A", 1225, 593, { size: 11, color: A_COL, style: "SemiBold" });
    text(C, "XSC1", 1092, 606, { size: 13, style: "SemiBold", color: INK });

    // ---------- floating oscilloscope ----------
    const SH = [{ type: "dropShadow", color: "#00000040", offset: [0, 14], radius: 36, spread: 0 }];
    const W = frame(C, "XSC1 · Oscilloscope", 414, 40, 576, 468, { fill: V("bg/card"), cornerRadius: 12, stroke: V("rule"), strokeWeight: 1, effects: SH });
    text(W, "XSC1", 14, 11, { mono: true, size: 12, style: "Medium", color: "text/secondary" });
    text(W, "Oscilloscope", 56, 10, { size: 13.5, style: "SemiBold" });
    rtext(W, "Wien oscillator · start-up", 520, 12, { mono: true, size: 11, color: "text/muted" });
    icon(W, "close", "ti:x", 543, 10, "text/muted", 1.4, 16);
    const S = frame(W, "Screen", 12, 38, 552, 220, { fill: "#0E0F11", cornerRadius: 8 });
    {
      const gw = 552 / 10, gh = 220 / 8;
      let g = ""; for (let i = 1; i < 10; i++) g += `M${(i * gw).toFixed(1)} 0V220 `; for (let j = 1; j < 8; j++) g += `M0 ${(j * gh).toFixed(1)}H552 `;
      vec(S, "graticule", g, 0, 0, { stroke: "#FFFFFF12", strokeWeight: 1 });
      let tk = ""; for (let i = 0; i <= 50; i++) tk += `M${(i * gw / 5).toFixed(1)} 107V113 `; for (let j = 0; j <= 40; j++) tk += `M273 ${(j * gh / 5).toFixed(1)}H279 `;
      vec(S, "axes", `M276 0V220 M0 110H552 ` + tk, 0, 0, { stroke: "#FFFFFF2E", strokeWeight: 1 });
      const cyc = 1.45; // cycles per div at 2 ms/div, f ≈ 723 Hz
      const env = (t) => (t < 0.4 ? 0.12 : Math.min(1, 0.12 + (t - 0.4) * 0.3)) ; // start-up envelope, t in div
      const tr = (amp, phase) => { let d = ""; for (let px = 0; px <= 552; px += 1.5) { const t = px / gw; const y = 110 - amp * gh * env(t) * Math.sin(2 * Math.PI * cyc * t + phase); d += `${px ? "L" : "M"}${px.toFixed(1)} ${y.toFixed(1)} `; } return d; };
      vec(S, "trace B", tr(1.0, 0), 0, 0, { stroke: B_COL, strokeWeight: 1.5, strokeJoin: "round" });
      vec(S, "trace A", tr(3.0, 0), 0, 0, { stroke: "#5B8DF6", strokeWeight: 1.5, strokeJoin: "round" });
      const t1 = 2 * gw, t2 = 7.5 * gw;
      vec(S, "T1", `M${t1} 0V220`, 0, 0, { stroke: "#FF8A5CCC", strokeWeight: 1, dashes: [4, 3] });
      vec(S, "T2", `M${t2} 0V220`, 0, 0, { stroke: "#9DBDFFCC", strokeWeight: 1, dashes: [4, 3] });
      vec(S, "T1 flag", `M${t1 - 6} 0 h12 l-6 8 z`, 0, 0, { fill: "#FF8A5C" });
      vec(S, "T2 flag", `M${t2 - 6} 0 h12 l-6 8 z`, 0, 0, { fill: "#9DBDFF" });
      text(S, "1", t1 + 5, 6, { mono: true, size: 10, color: "#FF8A5C" }); text(S, "2", t2 + 5, 6, { mono: true, size: 10, color: "#9DBDFF" });
      text(S, "A", 6, 20, { mono: true, size: 10, color: "#5B8DF6" }); text(S, "B", 6, 196, { mono: true, size: 10, color: B_COL });
    }
    // cursor table + actions
    const ct = frame(W, "Cursors", 12, 268, 400, 74, { fill: V("bg/inset"), cornerRadius: 8 });
    const cx = [10, 70, 170, 280];
    [["", "Time", "Channel A", "Channel B"], ["T1", "4.000 ms", "0.412 V", "0.137 V"], ["T2", "15.000 ms", "2.986 V", "0.995 V"], ["T2−T1", "11.000 ms", "2.574 V", "0.858 V"]].forEach((r, ri) =>
      r.forEach((c, ci) => text(ct, c, cx[ci], 6 + ri * 16, { mono: ri > 0, size: ri === 0 ? 10.5 : 11, color: ri === 0 || ci === 0 ? "text/muted" : ci === 2 ? "text/primary" : "text/secondary" })));
    btn(W, "Reverse", 422, 268, 70, false, null, 28);
    btn(W, "Save", 498, 268, 66, false, null, 28);
    const rg = (parent, label, x, y, on) => { ell(parent, "radio", x, y + 2, 12, 12, { fill: on ? V("text/primary") : "none", stroke: V("text/muted"), strokeWeight: 1.2 }); text(parent, label, x + 18, y, { size: 12, color: "text/secondary" }); };
    rg(W, "GND", 424, 314, true); rg(W, "Ext trig", 484, 314, false);
    // control groups
    const field = (p, k, v, x, y, w) => { text(p, k, x, y + 6, { size: 11, color: "text/muted" }); const f = frame(p, "field " + k, x + 46, y, w - 46, 24, { fill: V("bg/inset"), cornerRadius: 6, stroke: V("rule"), strokeWeight: 1 }); text(f, v, 8, 5, { mono: true, size: 11 }); };
    const seg = (p, items, x, y, w, act) => { const f = frame(p, "seg", x, y, w, 24, { fill: V("bg/inset"), cornerRadius: 6 }); const iw = (w - 4) / items.length; items.forEach((s, i) => { if (i === act) frame(f, "on", 2 + i * iw, 2, iw, 20, { fill: V("bg/raised"), cornerRadius: 5 }); text(f, s, 2 + i * iw, 5, { mono: true, size: 10, style: i === act ? "SemiBold" : "Regular", color: i === act ? "text/primary" : "text/secondary", w: iw, align: "center" }); }); };
    const grp = (title, x, w, sw, rows) => {
      const g = frame(W, "group " + title, x, 352, w, 104, { fill: "none", cornerRadius: 8, stroke: V("rule"), strokeWeight: 1 });
      if (sw) ell(g, "sw", 10, 12, 7, 7, { fill: sw });
      text(g, title, sw ? 22 : 10, 8, { size: 11.5, style: "SemiBold" });
      rows(g, w);
    };
    grp("Timebase", 12, 136, null, (g, w) => { field(g, "Scale", "2 ms/Div", 8, 30, w - 16); field(g, "X pos", "0", 8, 56, w - 16); seg(g, ["Y/T", "Add", "B/A", "A/B"], 8, 82 - 2, w - 16, 0); });
    grp("Channel A", 154, 132, "#5B8DF6", (g, w) => { field(g, "Scale", "1 V/Div", 8, 30, w - 16); field(g, "Y pos", "0", 8, 56, w - 16); seg(g, ["AC", "0", "DC"], 8, 80, w - 16, 2); });
    grp("Channel B", 292, 132, B_COL, (g, w) => { field(g, "Scale", "1 V/Div", 8, 30, w - 16); field(g, "Y pos", "0", 8, 56, w - 16); seg(g, ["AC", "0", "DC"], 8, 80, w - 16, 2); });
    grp("Trigger", 430, 134, null, (g, w) => { seg(g, ["↑", "↓", "A", "B", "Ext"], 8, 30, w - 16, 0); field(g, "Level", "0 V", 8, 56, w - 16); seg(g, ["Sing", "Nor", "Auto", "None"], 8, 80, w - 16, 2); });

    // ---------- instruments strip ----------
    const IS = frame(root, "Instruments", 1380, 92, 52, 718, { fill: V("bg/panel"), cornerRadius: 10 });
    inst.forEach(([name, d], i) => {
      const placed = name === "oscilloscope";
      const t = frame(IS, "inst/" + name, 8, 10 + i * 44, 36, 36, { fill: placed ? V("bg/raised") : "none", cornerRadius: 8, clip: false });
      icon(t, "icon", d, 8, 8, placed ? "text/primary" : "text/secondary", 1.4, 20);
      if (placed) ell(t, "placed", 29, 2, 6, 6, { fill: V("text/primary") });
    });
    rect(IS, "divider", 14, 10 + inst.length * 44 + 2, 24, 1, { fill: V("rule") });
    const pr = frame(IS, "inst/probe", 8, 10 + inst.length * 44 + 10, 36, 36, { fill: "none", cornerRadius: 8 });
    icon(pr, "icon", "ti:needle", 8, 8, "text/secondary", 1.4, 20);

    // ---------- design tabs ----------
    const tabs = [["Wien oscillator", true, false], ["Relay driver", false, true], ["Seat harness · sheet 1", false, false], ["VCO core", false, false]];
    let ttx = 8;
    for (const [s, act, dirty] of tabs) {
      const w = Math.round(s.length * 7.2 + (dirty ? 44 : 32));
      const t = frame(root, "tab " + s, ttx, 818, w, 32, { fill: act ? V("bg/panel") : "none", cornerRadius: 8, ...(act ? {} : { stroke: V("rule"), strokeWeight: 1 }) });
      text(t, s, 14, 8, { size: 12.5, style: act ? "SemiBold" : "Regular", color: act ? "text/primary" : "text/secondary" });
      if (dirty) ell(t, "unsaved", w - 18, 13, 6, 6, { fill: V("text/secondary") });
      ttx += w + 6;
    }
    const plus = frame(root, "new tab", ttx, 818, 32, 32, { fill: "none", cornerRadius: 8 });
    icon(plus, "plus", "ti:plus", 8, 8, "text/secondary", 1.5, 16);
    const toggles = [["Toolbox", "⌘1"], ["Spreadsheet", "⌘2"], ["Results", "⌘3"]];
    let gx = 1432;
    for (const [s, k] of toggles.reverse()) {
      const w = s.length * 7 + 46; gx -= w;
      const t = frame(root, "panel " + s, gx, 818, w, 32, { fill: "none", cornerRadius: 8, stroke: V("rule"), strokeWeight: 1 });
      text(t, s, 12, 8, { size: 12, color: "text/secondary" }); text(t, k, w - 30, 9, { mono: true, size: 10.5, color: "text/muted" });
      gx -= 6;
    }
    // ---------- status bar ----------
    const sb = frame(root, "Status bar", 0, 860, 1440, 40, { fill: V("bg/panel") });
    rect(sb, "rule", 0, 0, 1440, 1, { fill: V("rule") });
    text(sb, "Ready · F1 for help", 14, 12, { size: 12, color: "text/secondary" });
    text(sb, "A adjusts R3 · Shift+A lowers it · Space pauses", 220, 12, { size: 12, color: "text/muted" });
    mark(sb, "ok", 760, 15); text(sb, "ERC clean", 778, 12, { size: 12, color: "text/secondary" });
    text(sb, "Tran: 0.093 s     grid 2.54     zoom 100%     11 parts · 9 nets", 826, 12, { mono: true, size: 11.5, color: "text/secondary", w: 600, align: "right" });
  }

  // =====================================================================
  if (screen === "home") {
    const mb = menuBar(root);
    winCtl(mb, "Omnia");
    const SB = frame(root, "Sidebar", 12, 44, 288, 844, { fill: V("bg/panel"), cornerRadius: 14 });
    const wmk = wordmarkPaths(16, 44, 32);
    vec(SB, "Omnia wordmark · globe", wmk.globe, 0, 0, { stroke: V("text/primary"), strokeWeight: wmk.sw, strokeCap: "round", strokeJoin: "round" });
    vec(SB, "Omnia wordmark · mnia", wmk.letters, 0, 0, { fill: V("text/primary") });
    text(SB, "capture · simulate", Math.round(16 + wmk.width + 12), 30, { size: 11.5, color: "text/muted" });
    const search = frame(SB, "Search", 14, 68, 260, 38, { fill: V("bg/inset"), cornerRadius: 10, stroke: V("rule"), strokeWeight: 1 });
    icon(search, "search", "ti:search", 12, 11, "text/secondary", 1.4, 16);
    text(search, "Search designs and parts", 38, 11, { color: "text/muted" });
    btn(SB, "New design", 14, 118, 260, true, "⌘N", 38);
    const nav = (y, name, d, o = {}) => {
      if (o.sel) frame(SB, "selected", 14, y - 8, 260, 34, { fill: V("bg/raised"), cornerRadius: 8 });
      icon(SB, name, d, 22, y - 1, o.sel ? "text/primary" : "text/secondary", 1.4, 18);
      text(SB, name, 52, y, { size: 14, style: o.sel ? "SemiBold" : "Regular" });
      if (o.n) pill(SB, o.n, 244, y);
    };
    nav(186, "Home", "ti:home", { sel: true });
    nav(222, "Recent", "ti:clock");
    nav(258, "Templates", "ti:layout-grid", { n: "24" });
    nav(294, "Part libraries", "ti:books", { n: "3" });
    nav(330, "Simulation results", "ti:chart-line");
    rect(SB, "divider", 14, 366, 260, 1, { fill: V("rule") });
    text(SB, "Projects", 18, 382, { size: 12, color: "text/muted" });
    const proj = (y, s, n) => { icon(SB, "folder", "ti:folder", 22, y - 1, "text/secondary", 1.4, 18); text(SB, s, 52, y, { size: 14 }); pill(SB, n, 244, y); };
    proj(410, "PS-87 power seat", "3"); proj(446, "Bench PSU", "5"); proj(482, "Synth VCO", "2"); proj(518, "Oscillator studies", "4");
    text(SB, "Instruments", 18, 566, { size: 12, color: "text/muted" });
    text(SB, "14 virtual instruments, from multimeter to network analyzer. Drag one onto a sheet to wire it in.", 18, 588, { size: 12.5, color: "text/secondary", w: 252, lh: "150%" });
    const acct = frame(SB, "Account", 14, 782, 260, 48, { fill: V("bg/inset"), cornerRadius: 10 });
    ell(acct, "avatar", 10, 10, 28, 28, { fill: V("bg/raised") });
    icon(acct, "person", "ti:user", 15, 15, "text/secondary", 1.4, 18);
    text(acct, "Local library", 48, 8, { size: 13, style: "SemiBold" });
    text(acct, "offline · last synced 2 h ago", 48, 26, { size: 11, color: "text/muted" });

    const MX = 312, MW = 1116;
    const M = frame(root, "Home", MX, 44, MW, 844, { fill: V("bg/panel"), cornerRadius: 14 });
    text(M, "Recent designs", 28, 26, { size: 22, style: "SemiBold" });
    text(M, "Pick up where you left off, or start from a template.", 28, 58, { size: 13.5, color: "text/secondary" });
    const vs = frame(M, "View", MW - 176, 28, 148, 32, { fill: V("bg/inset"), cornerRadius: 9 });
    frame(vs, "on", 3, 3, 71, 26, { fill: V("bg/raised"), cornerRadius: 7 });
    text(vs, "Grid", 3, 7, { size: 12, style: "SemiBold", w: 71, align: "center" }); text(vs, "List", 74, 7, { size: 12, color: "text/secondary", w: 71, align: "center" });
    const cw = Math.floor((MW - 56 - 32) / 3), chh = 236;
    const chains = {
      wien: [["Source-COM-AC"], ["Resistor-IEEE-Standard"], ["Capacitor-IEEE-NonPolarized"], ["IC-COM-OpAmp", 50]],
      relay: [["Source-COM-Battery-Single"], ["Switch-COM-SPST"], ["Resistor-IEEE-Standard"], ["Diode-COM-LED"]],
      harness: [["Source-COM-DC"], ["Fuse-IEEE"], ["Switch-COM-SPST"], ["Resistor-IEEE-Standard"]],
      buck: [["Source-COM-DC"], ["Switch-COM-SPST"], ["Inductor-COM-Air"], ["Capacitor-IEEE-NonPolarized"]],
      vco: [["Capacitor-IEEE-NonPolarized"], ["Resistor-IEEE-Standard"], ["IC-COM-Schmitt"], ["Miscellaneous-COM-Crystal_Oscillator"]],
      psu: [["Source-COM-AC"], ["Diode-COM-Standard"], ["Capacitor-IEEE-NonPolarized"], ["Resistor-IEEE-Standard"]],
    };
    const thumbs = {
      wien: "M20 80h60 M80 70l20 10l-20 10 M100 80h40 M140 64v32 M150 64v32 M150 80h40 M190 50l50 30l-50 30z M240 80h50 M265 80v-40h-90v40",
      relay: "M20 40h250 M60 40v80 M150 40v30 M140 70h20v40h-20z M150 110v20 M230 40v30 M230 70l-6 30 M230 100v30 M20 140h250",
      harness: "M30 40h50v100H30z M80 60h80 M80 80h120 M80 100h60 M80 120h150 M160 60v-20h60 M200 80v40",
      buck: "M20 50h60 M80 50l20 0 M100 50c5-12 15-12 20 0c5-12 15-12 20 0c5-12 15-12 20 0 M160 50h110 M100 50v60 M200 50v60 M190 110h20 M190 120h20 M20 140h250",
      vco: "M30 60h40 M70 40l40 20l-40 20z M110 60h40 M150 40h40v40h-40z M190 60h60 M250 60v60h-200v-60",
      psu: "M20 60h40 M60 45v30 M70 45v30 M70 60h40 M110 40h60v40h-60z M170 60h90 M200 60v50 M190 110h20 M195 116h10",
    };
    const cards = [
      ["Wien oscillator", "wien", "open · simulating", "Oscillator studies", "measured"],
      ["Relay driver", "relay", "edited 2 h ago", "PS-87 power seat", "conflict"],
      ["Seat harness · sheet 1", "harness", "edited yesterday", "PS-87 power seat", "documented"],
      ["Buck 12 → 5 V", "buck", "edited 3 days ago", "Bench PSU", "ok"],
      ["VCO core", "vco", "edited last week", "Synth VCO", "open"],
      ["Linear regulator", "psu", "edited 2 weeks ago", "Bench PSU", "ok"],
    ];
    cards.forEach(([name, th, meta, proj_, kind], i) => {
      const x = 28 + (i % 3) * (cw + 16), y = 96 + Math.floor(i / 3) * (chh + 16);
      const c = frame(M, "card " + name, x, y, cw, chh, { fill: V("bg/card"), cornerRadius: 12, stroke: i === 0 ? V("text/muted") : V("rule"), strokeWeight: 1 });
      const t = frame(c, "thumb", 8, 8, cw - 16, 152, { fill: V("stage/paper"), cornerRadius: 8 });
      { const chain = chains[th], S = 64, x0 = Math.round((cw - 16 - chain.length * S) / 2), y0 = 40, py = y0 + S / 2;
        chain.forEach(([nm, inY], j) => { placeSym(t, nm, { x: x0 + j * S, y: y0, size: S }, INK, 1.6, nm); if (inY !== undefined) vec(t, "jog", `M${x0 + j * S} ${py} V${y0 + inY * S / 150}`, 0, 0, { stroke: INK, strokeWeight: 1.6 }); });
        vec(t, "return", `M${x0} ${py} V120 H${x0 + chain.length * S} V${py}`, 0, 0, { stroke: INK, strokeWeight: 1.6, strokeJoin: "round" }); }
      if (i === 0) { const lv = frame(t, "live", cw - 16 - 76, 10, 66, 22, { fill: "#111112", cornerRadius: 11 }); ell(lv, "dot", 9, 8, 6, 6, { fill: "#5FD4A4" }); text(lv, "running", 19, 4, { mono: true, size: 10, color: "#F2F2F2" }); }
      mark(c, kind, 14, 177);
      text(c, name, 32, 172, { size: 14, style: "SemiBold" });
      text(c, proj_, 14, 198, { size: 12, color: "text/secondary" });
      rtext(c, meta, cw - 14, 199, { mono: true, size: 11, color: "text/muted" });
    });
    text(M, "Start from a template", 28, 618, { size: 15, style: "SemiBold" });
    rtext(M, "All 24 templates →", MW - 28, 620, { size: 12.5, color: "text/secondary" });
    const tpl = [["Op-amp filter", null, "IC-COM-OpAmp"], ["Relay driver", null, "Relay-IEEE-SPST-NO"], ["Buck converter", null, "Inductor-COM-Air"], ["555 timer", "ti:cpu-2", null], ["H-bridge", null, "Transistor-COM-MOSFET-N-Enhancement"], ["Wien oscillator", null, "Source-COM-AC"]];
    const tw = Math.floor((MW - 56 - 5 * 12) / 6);
    tpl.forEach(([s, d, sy], i) => {
      const t = frame(M, "template " + s, 28 + i * (tw + 12), 652, tw, 110, { fill: V("bg/inset"), cornerRadius: 12, stroke: V("rule"), strokeWeight: 1 });
      const ib = frame(t, "icon", 14, 14, 40, 40, { fill: V("bg/raised"), cornerRadius: 10 });
      if (sy) placeSym(ib, sy, { x: 6, y: 6, size: 28 }, "text/primary", 1.5, "symbol"); else icon(ib, "glyph", d, 10, 10, "text/primary", 1.5, 20);
      text(t, s, 14, 68, { size: 13.5, style: "SemiBold" });
      text(t, "template", 14, 88, { mono: true, size: 10.5, color: "text/muted" });
    });
    text(M, "Tip: press A or Shift+A while a sheet is running to turn an interactive potentiometer.", 28, 798, { size: 12.5, color: "text/muted" });
  }
  // =====================================================================
  if (screen === "first") {
    // frameless window: just the controls
    const tb = frame(root, "Title bar", 0, 0, 1440, 36, { fill: "none" });
    icon(tb, "min", "ti:minus", 1348, 10, "text/secondary", 1.3, 16);
    icon(tb, "max", "ti:square", 1378, 10, "text/secondary", 1.3, 16);
    icon(tb, "close", "ti:x", 1408, 10, "text/secondary", 1.3, 16);

    // ---------- hero ----------
    const H = frame(root, "Hero", 12, 12, 588, 876, { fill: V("bg/panel"), cornerRadius: 18 });
    // a very large, faint globe bleeding off the panel
    vec(H, "globe backdrop", transformD(OMNIA_MARK, { x: 40, y: 150, size: 760 }), 0, 0, { stroke: V("rule"), strokeWeight: 6, strokeJoin: "round" });
    vec(H, "globe backdrop 2", transformD(OMNIA_MARK, { x: 40, y: 150, size: 760 }), 0, 0, { stroke: V("highlight"), strokeWeight: 1.2, strokeJoin: "round" });
    const wm = wordmarkPaths(48, 132, 64);
    vec(H, "Omnia wordmark · globe", wm.globe, 0, 0, { stroke: V("text/primary"), strokeWeight: wm.sw, strokeCap: "round", strokeJoin: "round" });
    vec(H, "Omnia wordmark · mnia", wm.letters, 0, 0, { fill: V("text/primary") });
    text(H, "Capture, simulate and measure circuits.", 50, 160, { size: 18, color: "text/secondary" });
    // three promises
    const promise = (y, ti, title, body) => {
      const b = frame(H, "icon " + title, 50, y, 40, 40, { fill: V("bg/raised"), cornerRadius: 10 });
      icon(b, "icon", ti, 10, 10, "text/primary", 1.5, 20);
      text(H, title, 106, y + 1, { size: 15, style: "SemiBold" });
      text(H, body, 106, y + 22, { size: 13, color: "text/secondary", w: 420 });
    };
    promise(560, "ti:route", "Draw with real symbols", "IEEE or IEC parts, wires that snap to pins, live ERC.");
    promise(626, "ti:device-desktop-analytics", "Simulate as you draw", "Transient, AC and DC runs with virtual instruments on the sheet.");
    promise(692, "ti:gauge", "Measure against the model", "Compare bench readings to the simulation, part by part.");
    text(H, "v0.1.0 · first run", 50, 836, { mono: true, size: 11.5, color: "text/muted" });
    rtext(H, "Local · nothing leaves this machine", 556, 836, { size: 11.5, color: "text/muted" });

    // ---------- setup ----------
    const SX = 640, SW = 760;
    text(root, "Set up Omnia", SX, 52, { size: 26, style: "SemiBold" });
    text(root, "Four choices, all changeable later in Settings.", SX, 88, { size: 14, color: "text/secondary" });
    const section = (n, y, title, hint) => {
      const b = frame(root, "step " + n, SX, y, 24, 24, { fill: V("bg/raised"), cornerRadius: 12 });
      text(b, String(n), 0, 4, { mono: true, size: 11.5, style: "Medium", w: 24, align: "center" });
      text(root, title, SX + 36, y + 2, { size: 15, style: "SemiBold" });
      if (hint) rtext(root, hint, SX + SW, y + 4, { size: 12, color: "text/muted" });
    };
    const choice = (name, x, y, w, h, on) => frame(root, name, x, y, w, h, { fill: V("bg/card"), cornerRadius: 12, stroke: on ? V("text/primary") : V("rule"), strokeWeight: on ? 1.5 : 1 });
    const radio = (p, x, y, on) => { ell(p, "radio", x, y, 16, 16, { fill: "none", stroke: on ? V("text/primary") : V("text/muted"), strokeWeight: 1.4 }); if (on) ell(p, "dot", x + 4, y + 4, 8, 8, { fill: V("text/primary") }); };

    // 1 · Appearance
    section(1, 136, "Appearance", "follows the system until you pick");
    const preview = (p, x, y, pal) => { // tiny workspace thumbnail in fixed colours
      const f = frame(p, "preview", x, y, 118, 84, { fill: pal.bg, cornerRadius: 6 });
      frame(f, "toolbar", 0, 0, 118, 12, { fill: pal.panel });
      frame(f, "canvas", 6, 18, 90, 60, { fill: "#EFEDE6", cornerRadius: 3 });
      vec(f, "wires", "M12 40h22 M44 40h40 M84 40v22h-72v-22 M34 34v12 M40 34v12", 0, 0, { stroke: "#1D1D1F", strokeWeight: 1 });
      frame(f, "scope", 50, 24, 40, 26, { fill: "#0E0F11", cornerRadius: 2 });
      vec(f, "trace", "M53 37c3-8 5-8 7 0s5 8 7 0s5-8 7 0s4 8 7 0", 0, 0, { stroke: "#5B8DF6", strokeWeight: 0.8 });
      frame(f, "strip", 102, 18, 10, 60, { fill: pal.panel, cornerRadius: 3 });
    };
    [["Graphite", { bg: "#0B0B0C", panel: "#161617" }, "Dark, high contrast"], ["Gray", { bg: "#C4C4C4", panel: "#D3D3D3" }, "Soft light gray"]].forEach(([name, pal, sub], i) => {
      const on = name === theme;
      const c = choice("theme " + name, SX + i * 254, 172, 242, 112, on);
      preview(c, 12, 14, pal);
      radio(c, 144, 16, on);
      text(c, name, 144, 44, { size: 14, style: "SemiBold" });
      text(c, sub, 144, 64, { size: 11.5, color: "text/secondary", w: 90 });
    });
    {
      const c = choice("theme System", SX + 2 * 254, 172, 252, 112, false);
      radio(c, 16, 16, false);
      text(c, "Match system", 42, 15, { size: 14, style: "SemiBold" });
      text(c, "Graphite at night, Gray by day, following Windows.", 16, 44, { size: 12, color: "text/secondary", w: 220 });
    }

    // 2 · Symbols
    section(2, 306, "Schematic symbols", "applies to new sheets");
    [["IEEE · ANSI", "US style, zigzag resistors", "Resistor-IEEE-Standard", "Capacitor-IEEE-NonPolarized", true], ["IEC 60617", "European style, box resistors", "Resistor-IEC-Standard", "Capacitor-IEC-NonPolarized", false]].forEach(([name, sub, r, cap, on], i) => {
      const c = choice("symbols " + name, SX + i * 384, 342, 376, 96, on);
      const pv = frame(c, "symbols", 12, 12, 150, 72, { fill: "#EFEDE6", cornerRadius: 6 });
      placeSym(pv, r, { x: 6, y: 6, size: 60 }, "#1D1D1F", 1.6, "resistor");
      placeSym(pv, cap, { x: 80, y: 6, size: 60 }, "#1D1D1F", 1.6, "capacitor");
      radio(c, 180, 18, on);
      text(c, name, 206, 17, { size: 14, style: "SemiBold" });
      text(c, sub, 180, 46, { size: 12, color: "text/secondary", w: 180 });
    });

    // 3 · Libraries
    section(3, 462, "Part libraries", "3.2 GB free on C:");
    const toggle = (p, x, y, on, locked) => {
      const t = frame(p, "toggle", x, y, 36, 20, { fill: on ? V("text/primary") : V("bg/raised"), cornerRadius: 10, opacity: locked ? 0.5 : 1 });
      ell(t, "knob", on ? 18 : 2, 2, 16, 16, { fill: on ? V("bg/panel") : V("text/muted") });
    };
    const LB = frame(root, "libraries", SX, 498, SW, 148, { fill: V("bg/card"), cornerRadius: 12, stroke: V("rule"), strokeWeight: 1 });
    const lib = (i, ti, name, meta, right, on, locked) => {
      const y = 12 + i * 46;
      if (i) rect(LB, "rule", 16, y - 6, SW - 32, 1, { fill: V("rule") });
      icon(LB, "icon", ti, 16, y + 6, "text/secondary", 1.4, 20);
      text(LB, name, 48, y + 2, { size: 13.5, style: "SemiBold" });
      text(LB, meta, 48, y + 21, { size: 12, color: "text/secondary" });
      rtext(LB, right, SW - 70, y + 10, { mono: true, size: 11.5, color: on ? "text/secondary" : "text/muted" });
      toggle(LB, SW - 52, y + 8, on, locked);
    };
    lib(0, "ti:components", "Omnia basics", "116 IEEE and IEC symbols · MIT · ships with the app", "installed", true, true);
    lib(1, "ti:books", "KiCad symbol library", "About 20,000 parts with pin maps · CC-BY-SA 4.0", "download · 180 MB", false, false);
    lib(2, "ti:folder-open", "Your own parts", "Point Omnia at a folder of .kicad_sym or .omnia-lib files", "choose folder…", false, false);

    // 4 · Simulator
    section(4, 670, "Simulator", "Use another SPICE engine…");
    const SM = frame(root, "simulator", SX, 706, SW, 108, { fill: V("bg/card"), cornerRadius: 12, stroke: V("rule"), strokeWeight: 1 });
    const engine = (i, name, meta, tag, on, kind) => {
      const y = 12 + i * 48;
      if (i) rect(SM, "rule", 16, y - 6, SW - 32, 1, { fill: V("rule") });
      radio(SM, 18, y + 10, on);
      text(SM, name, 46, y + 2, { size: 13.5, style: "SemiBold" });
      text(SM, meta, 46, y + 21, { size: 12, color: "text/secondary" });
      const tw = tag.length * 7 + 18;
      const t = frame(SM, "tag " + tag, SW - 16 - tw - (kind ? 22 : 0), y + 8, tw, 22, { fill: "none", cornerRadius: 11, stroke: V("rule"), strokeWeight: 1 });
      text(t, tag, 0, 4, { mono: true, size: 11, color: "text/secondary", w: tw, align: "center" });
      if (kind) mark(SM, kind, SW - 28, y + 13);
    };
    engine(0, "ngspice", "Bundled · tested on this machine in 0.4 s", "stable", true, "ok");
    engine(1, "RIS-C", "Omnia's own SPICE engine, written in Rust · in development", "preview", false, "later");

    // footer
    text(root, "Press Enter to accept these defaults.", SX, 852, { size: 12.5, color: "text/muted" });
    btn(root, "Open a sample", SX + SW - 316, 840, 140, false, null, 40);
    btn(root, "Start designing", SX + SW - 166, 840, 166, true, "↵", 40);
  }

  return cmds;
}

const script = [
  { command: "page.add", params: { name: "Screens" } },
  { command: "command.batch", params: { label: "Workspace · Graphite", commands: build("Graphite", 0, "work", 0) } },
  { command: "command.batch", params: { label: "Workspace · Gray", commands: build("Gray", 1540, "work", 0) } },
  { command: "command.batch", params: { label: "Home · Graphite", commands: build("Graphite", 0, "home", 1000) } },
  { command: "command.batch", params: { label: "Home · Gray", commands: build("Gray", 1540, "home", 1000) } },
  { command: "command.batch", params: { label: "First boot · Graphite", commands: build("Graphite", 0, "first", 2000) } },
  { command: "command.batch", params: { label: "First boot · Gray", commands: build("Gray", 1540, "first", 2000) } },
  { command: "page.add", params: { name: "Components" } },
];
writeFileSync(new URL("./capture4.json", import.meta.url), JSON.stringify(script));
console.log("ok", script.slice(1, 7).reduce((a, s) => a + s.params.commands.length, 0), "commands");
