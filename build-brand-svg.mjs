import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import opentype from "opentype.js";
import { transformD } from "./symbols.mjs";
const OUT = fileURLToPath(new URL("./brand/", import.meta.url));
const FONT = opentype.loadSync(fileURLToPath(new URL("./vendor/asimovian/fonts/ttf/Asimovian-Regular.ttf", import.meta.url)));
let MARK = ""; for (let i = 0; i <= 900; i++) { const t = (i / 900) * 2 * Math.PI; const x = Math.sin(3 * t + Math.PI / 2), y = Math.sin(4 * t); const X = x * Math.sqrt(1 - y * y / 2), Y = y * Math.sqrt(1 - x * x / 2); MARK += `${i ? "L" : "M"}${(75 + 56 * X).toFixed(2)} ${(75 + 56 * Y).toFixed(2)}`; }
MARK += "Z";
const svg = (vb, body, title) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vb}" role="img" aria-label="${title}">\n<title>${title}</title>\n${body}\n</svg>\n`;
const markEl = (d, sw, color) => `<path d="${d}" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linejoin="round" stroke-linecap="round"/>`;
// 1. mark alone (currentColor so it takes the surrounding text colour)
writeFileSync(OUT + "omnia-mark.svg", svg("12 12 126 126", markEl(MARK, 7, "currentColor"), "Omnia"));
writeFileSync(OUT + "omnia-mark-black.svg", svg("12 12 126 126", markEl(MARK, 7, "#111112"), "Omnia"));
writeFileSync(OUT + "omnia-mark-white.svg", svg("12 12 126 126", markEl(MARK, 7, "#F2F2F2"), "Omnia"));
// 2. app icon: rounded square, mark at the menu-bar proportions (stroke 1.15 at 22 px)
const iconMark = transformD(MARK, { x: 112, y: 112, size: 800 }); // globe spans ~58% of the tile
const icon = (bg, fg) => svg("0 0 1024 1024", `<rect width="1024" height="1024" rx="280" fill="${bg}"/>\n${markEl(iconMark, 44, fg)}`, "Omnia");
writeFileSync(OUT + "omnia-app-icon.svg", icon("#F2F2F2", "#111112"));
writeFileSync(OUT + "omnia-app-icon-dark.svg", icon("#111112", "#F2F2F2"));
// 3. wordmark: Asimovian with the globe as the O (style C, −0.03 em spacing, +0.015 em drop)
function wordmark(color) {
  const size = 100, k = size / FONT.unitsPerEm, baseline = 0, x = 0;
  const bb = FONT.charToGlyph("O").getBoundingBox();
  const top = baseline - bb.y2 * k, bot = baseline - bb.y1 * k, D = (bot - top) * 1.04, sw = 116 * k * 0.5, sb = 50 * k;
  const S = ((D - sw) * 150) / 112, cx = x + sb + D / 2, cy = (top + bot) / 2 + 0.015 * size;
  const globe = transformD(MARK, { x: cx - S / 2, y: cy - S / 2, size: S });
  const p = FONT.getPath("mnia", x + sb + D + sb - 0.03 * size, baseline, size);
  const lb = p.getBoundingBox();
  const minX = Math.min(cx - D / 2, lb.x1) - 2, maxX = lb.x2 + 2, minY = Math.min(cy - D / 2, lb.y1) - 2, maxY = Math.max(cy + D / 2, lb.y2) + 2;
  const vb = `${minX.toFixed(2)} ${minY.toFixed(2)} ${(maxX - minX).toFixed(2)} ${(maxY - minY).toFixed(2)}`;
  return svg(vb, `${markEl(globe, sw.toFixed(2), color)}\n<path d="${p.toPathData(2)}" fill="${color}"/>`, "Omnia");
}
writeFileSync(OUT + "omnia-wordmark.svg", wordmark("currentColor"));
writeFileSync(OUT + "omnia-wordmark-black.svg", wordmark("#111112"));
writeFileSync(OUT + "omnia-wordmark-white.svg", wordmark("#F2F2F2"));
console.log("ok");
