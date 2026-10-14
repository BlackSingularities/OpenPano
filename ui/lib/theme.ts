// Interface themes. Every theme is derived from a few base colours; foreground colours
// (text, icons, active blocks) are chosen automatically for contrast.

export type ThemeId = "dark" | "light" | "gray" | "classic" | "custom";

export const THEMES: { id: ThemeId; desktopOnly: boolean; swatch: string }[] = [
  { id: "dark", desktopOnly: false, swatch: "#000000" },
  { id: "light", desktopOnly: false, swatch: "#ffffff" },
  { id: "gray", desktopOnly: true, swatch: "#3a3d42" },
  { id: "classic", desktopOnly: true, swatch: "#1a1d25" },
  { id: "custom", desktopOnly: true, swatch: "conic-gradient(#f44, #fd4, #4f8, #4af, #a4f, #f44)" },
];

type RGB = [number, number, number];

const hex = (c: string): RGB => {
  const h = c.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((x) => x + x).join("") : h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const toHex = (c: RGB) => "#" + c.map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
const mix = (a: RGB, b: RGB, t: number): RGB => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const lum = (c: RGB) => {
  const f = (v: number) => {
    const x = v / 255;
    return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
};

/** Black or white, whichever contrasts better with the colour. */
export function contrastOn(c: string): string {
  return lum(hex(c)) > 0.4 ? "#000000" : "#ffffff";
}

interface Base {
  surface: string; // main background
  accent?: string; // active blocks; defaults to the contrasting foreground
  raisedStep?: number;
}

function derive(b: Base): Record<string, string> {
  const s = hex(b.surface);
  const light = lum(s) > 0.4;
  const fg: RGB = light ? [0, 0, 0] : [255, 255, 255];
  const step = b.raisedStep ?? (light ? 0.05 : 0.06);
  const accent = b.accent ?? toHex(fg);
  const accentRgb = hex(accent);
  const selFg = contrastOn(accent);
  const fgs = fg.join(", ");
  return {
    "--bg": b.surface,
    "--bg-2": b.surface,
    "--panel": b.surface,
    "--panel-2": b.surface,
    "--surface": b.surface,
    "--raised": toHex(mix(s, fg, step)),
    "--raised-2": toHex(mix(s, fg, step * 2)),
    "--line": `rgba(${fgs}, ${light ? 0.14 : 0.14})`,
    "--line-2": `rgba(${fgs}, ${light ? 0.26 : 0.24})`,
    "--text": toHex(fg),
    "--text-2": `rgba(${fgs}, 0.72)`,
    "--text-3": `rgba(${fgs}, 0.45)`,
    "--fg-rgb": fgs,
    "--bg-rgb": s.join(", "),
    "--accent": accent,
    "--accent-2": accent,
    "--accent-ink": selFg,
    "--accent-soft": `rgba(${accentRgb.join(", ")}, 0.12)`,
    "--sel-bg": accent,
    "--sel-fg": selFg,
    "--sel-hover": toHex(mix(accentRgb, s, 0.12)),
    "--blue": accent,
    "--blue-soft": `rgba(${accentRgb.join(", ")}, 0.14)`,
    "--ok": light ? "#1a7f3c" : accent,
    "--warn": light ? "#9a6a00" : "#ffd34d",
    "--err": light ? "#d0242f" : "#ff3b47",
    "--shadow": light ? "0 12px 34px rgba(0,0,0,0.16)" : "0 12px 40px rgba(0,0,0,0.6)",
    "color-scheme": light ? "light" : "dark",
  };
}

export function themeVars(id: ThemeId, custom: string): Record<string, string> {
  switch (id) {
    case "light":
      return derive({ surface: "#ffffff" });
    case "gray":
      return derive({ surface: "#2c2f34", raisedStep: 0.07 });
    case "classic": {
      const v = derive({ surface: "#1a1d25", accent: "#ffaa3c", raisedStep: 0.05 });
      Object.assign(v, { "--bg": "#111318", "--surface": "#1a1d25", "--panel": "#1a1d25", "--panel-2": "#20242e", "--raised": "#262b37", "--raised-2": "#2e3442" });
      return v;
    }
    case "custom":
      return derive({ surface: custom || "#1e2a44" });
    default:
      return derive({ surface: "#000000" });
  }
}

export function applyTheme(id: ThemeId, custom: string) {
  const v = themeVars(id, custom);
  const root = document.documentElement.style;
  for (const [k, val] of Object.entries(v)) {
    if (k === "color-scheme") root.colorScheme = val;
    else root.setProperty(k, val);
  }
}

/** Preview background colour (RGB 0..1) for the WebGL renderer. */
export function canvasBg(): [number, number, number] {
  const s = getComputedStyle(document.documentElement).getPropertyValue("--bg-rgb").split(",").map((x) => +x.trim() / 255);
  return s.length === 3 && s.every((x) => isFinite(x)) ? (s as [number, number, number]) : [0, 0, 0];
}
