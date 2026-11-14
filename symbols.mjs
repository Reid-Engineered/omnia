// Loads Chris Pikul's electronic-symbols SVGs (MIT) and turns them into
// absolute, transformed path data for FigCraft vectors.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const DIR = fileURLToPath(new URL("./vendor/electronic-symbols/SVG/", import.meta.url));
const cache = new Map();

// ---- SVG element extraction: [{ d, filled }] ----
function load(name, file) {
  const key = file || name;
  if (cache.has(key)) return cache.get(key);
  const src = readFileSync(file || DIR + name + ".svg", "utf8");
  const out = [];
  const stack = [];
  const re = /<(\/?)(g|path|circle|ellipse|rect|line|polygon|polyline)\b([^>]*?)(\/?)>/g;
  let m;
  const attr = (s, k) => { const r = new RegExp(`\\s${k}="([^"]*)"`).exec(s); return r ? r[1] : undefined; };
  while ((m = re.exec(src))) {
    const [, close, tag, attrs, self] = m;
    if (tag === "g") { if (close) stack.pop(); else if (!self) stack.push(attr(attrs, "fill")); continue; }
    if (close || attr(attrs, "stroke") === "none") continue; // skip Tabler's invisible bounding box
    const fill = attr(attrs, "fill") ?? [...stack].reverse().find((f) => f !== undefined);
    const filled = fill !== "none";
    let d;
    const n = (k) => parseFloat(attr(attrs, k) ?? "0");
    if (tag === "path") d = attr(attrs, "d");
    else if (tag === "circle" || tag === "ellipse") {
      const cx = n("cx"), cy = n("cy"), rx = tag === "circle" ? n("r") : n("rx"), ry = tag === "circle" ? n("r") : n("ry");
      d = `M${cx - rx} ${cy}A${rx} ${ry} 0 1 0 ${cx + rx} ${cy}A${rx} ${ry} 0 1 0 ${cx - rx} ${cy}Z`;
    } else if (tag === "rect") d = `M${n("x")} ${n("y")}h${n("width")}v${n("height")}h${-n("width")}Z`;
    else if (tag === "line") d = `M${n("x1")} ${n("y1")}L${n("x2")} ${n("y2")}`;
    else d = "M" + attr(attrs, "points").trim().replace(/\s+/g, " ") + (tag === "polygon" ? "Z" : "");
    out.push({ d, filled });
  }
  cache.set(key, out);
  return out;
}

// ---- path normalisation to absolute M L C Q A Z ----
function parse(d) {
  const toks = d.match(/[a-zA-Z]|-?(?:\d*\.\d+|\d+\.?)(?:e[-+]?\d+)?/g);
  const segs = [];
  let i = 0, cmd = "", cx = 0, cy = 0, sx = 0, sy = 0, lc = null, lq = null;
  const num = () => parseFloat(toks[i++]);
  const flag = () => { // arc flags may be glued: "1 0" or "10"
    const t = toks[i];
    if (t.length > 1 && (t[0] === "0" || t[0] === "1") && !t.includes(".")) { toks[i] = t.slice(1); return +t[0]; }
    i++; return +t;
  };
  while (i < toks.length) {
    if (/[a-zA-Z]/.test(toks[i])) cmd = toks[i++];
    const rel = cmd === cmd.toLowerCase(), C = cmd.toUpperCase();
    const ox = rel ? cx : 0, oy = rel ? cy : 0;
    if (C === "Z") { segs.push(["Z"]); cx = sx; cy = sy; lc = lq = null; continue; }
    if (C === "M") { cx = ox + num(); cy = oy + num(); sx = cx; sy = cy; segs.push(["M", cx, cy]); cmd = rel ? "l" : "L"; lc = lq = null; continue; }
    if (C === "L") { cx = ox + num(); cy = oy + num(); segs.push(["L", cx, cy]); lc = lq = null; continue; }
    if (C === "H") { cx = ox + num(); segs.push(["L", cx, cy]); lc = lq = null; continue; }
    if (C === "V") { cy = (rel ? cy : 0) + num(); segs.push(["L", cx, cy]); lc = lq = null; continue; }
    if (C === "C" || C === "S") {
      let x1, y1;
      if (C === "C") { x1 = ox + num(); y1 = oy + num(); } else { x1 = lc ? 2 * cx - lc[0] : cx; y1 = lc ? 2 * cy - lc[1] : cy; }
      const x2 = ox + num(), y2 = oy + num(), x = ox + num(), y = oy + num();
      segs.push(["C", x1, y1, x2, y2, x, y]); lc = [x2, y2]; lq = null; cx = x; cy = y; continue;
    }
    if (C === "Q" || C === "T") {
      let x1, y1;
      if (C === "Q") { x1 = ox + num(); y1 = oy + num(); } else { x1 = lq ? 2 * cx - lq[0] : cx; y1 = lq ? 2 * cy - lq[1] : cy; }
      const x = ox + num(), y = oy + num();
      segs.push(["Q", x1, y1, x, y]); lq = [x1, y1]; lc = null; cx = x; cy = y; continue;
    }
    if (C === "A") {
      const rx = num(), ry = num(), rot = num(), la = flag(), sw = flag(), x = ox + num(), y = oy + num();
      segs.push(["A", rx, ry, rot, la, sw, x, y]); cx = x; cy = y; lc = lq = null; continue;
    }
    throw new Error("unsupported path command " + cmd);
  }
  return segs;
}

// ---- placement ----
// box = { x, y, size, rot (0|90|180|270, clockwise), flip (mirror X before rotating) }
function makeT({ x, y, size, rot = 0, flip = false }) {
  const k = size / 150;
  return (px, py) => {
    let u = (flip ? 150 - px : px) - 75, v = py - 75;
    for (let r = 0; r < rot; r += 90) [u, v] = [-v, u];
    return [x + (u + 75) * k, y + (v + 75) * k];
  };
}
const f = (n) => Math.round(n * 100) / 100;

export function symbolPaths(name, box) {
  const T = makeT(box), k = box.size / 150, mirror = !!box.flip;
  return load(name).map(({ d, filled }) => {
    const out = parse(d).map((s) => {
      if (s[0] === "Z") return "Z";
      if (s[0] === "A") {
        const [, rx, ry, r, la, sw, x, y] = s; const [X, Y] = T(x, y);
        return `A${f(rx * k)} ${f(ry * k)} ${f((mirror ? -r : r) + (box.rot || 0))} ${la} ${mirror ? 1 - sw : sw} ${f(X)} ${f(Y)}`;
      }
      const pts = []; for (let j = 1; j < s.length; j += 2) pts.push(...T(s[j], s[j + 1]).map(f));
      return s[0] + pts.join(" ");
    }).join(" ");
    return { d: out, filled };
  });
}
// pin position in the symbol's 150 box → placed coordinates
export function pinAt(box, px, py) { return makeT(box)(px, py).map(f); }

// transform arbitrary path data drawn in a 150 box (used for logo marks)
export function transformD(d, box) {
  const T = makeT(box), k = box.size / 150, mirror = !!box.flip;
  return parse(d).map((s) => {
    if (s[0] === "Z") return "Z";
    if (s[0] === "A") {
      const [, rx, ry, r, la, sw, x, y] = s; const [X, Y] = T(x, y);
      return `A${f(rx * k)} ${f(ry * k)} ${f((mirror ? -r : r) + (box.rot || 0))} ${la} ${mirror ? 1 - sw : sw} ${f(X)} ${f(Y)}`;
    }
    const pts = []; for (let j = 1; j < s.length; j += 2) pts.push(...T(s[j], s[j + 1]).map(f));
    return s[0] + pts.join(" ");
  }).join(" ");
}

// Tabler icon (MIT) → path data placed at x,y with the given pixel size (icons are drawn on a 24 grid)
const TABLER = fileURLToPath(new URL("./vendor/tabler-icons/outline/", import.meta.url));
export function tablerD(name, x, y, size) {
  return load(null, TABLER + name + ".svg").map(({ d }) => transformD(d, { x, y, size: (size * 150) / 24 })).join(" ");
}
