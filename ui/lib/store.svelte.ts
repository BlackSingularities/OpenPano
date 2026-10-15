// Global application state and actions.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";

export const IS_MOBILE = /android|iphone|ipad|ipod/i.test(navigator.userAgent);
import type { LensType, Projection } from "./geometry";
import { setLang, t, type Lang } from "./i18n.svelte";
import type { SphereMeta } from "./gl";
import { applyTheme, THEMES, type ThemeId } from "./theme";
import { kindOf, wrap180, type Kind } from "./composition";

export type CropMode = "Auto" | "None" | "Manual";
export type Blend = "Multiband" | "Feather" | "Seams";
export type Strategy = "Auto" | "AllPairs" | "Sequential";
export type OutFormat = "Auto" | "Jpeg" | "Tiff16" | "Png16" | "Dng";

export interface RenderSettings {
  auto_projection: boolean;
  projection: Projection;
  blend: Blend;
  bands: number;
  crop: CropMode;
  manual_crop: [number, number, number, number];
  full_sphere: boolean;
  scale: number;
  max_megapixels: number;
  yaw: number;
  pitch: number;
  roll: number;
  /** field of view of view-type projections, degrees (0 = auto) */
  fov: number;
  /** pitch/roll that level the horizon (UI only, see lib/composition.ts) */
  level_pitch: number;
  level_roll: number;
  circular_fisheye: boolean;
  bicubic: boolean;
  flip_h: boolean;
  flip_v: boolean;
}

export interface Settings {
  lang: Lang;
  align: {
    lens: LensType;
    manual_hfov: number | null;
    max_features: number;
    strategy: Strategy;
    window: number;
    optimize_focal: boolean;
    straighten: boolean;
    ransac_threshold_px: number;
  };
  manualFov: boolean;
  fovDeg: number;
  photo: { exposure: boolean; color: boolean; vignetting: boolean };
  render: RenderSettings;
  output: { format: OutFormat; jpeg_quality: number; transparent: boolean; gpano: boolean };
  ui: { showGrid: boolean; showOutlines: boolean; showNav: boolean; quality: "fast" | "normal" | "high"; revealAfter: boolean; theme: ThemeId; customColor: string; checkUpdates: boolean; skippedVersion: string };
}

export const defaultRender = (): RenderSettings => ({
  auto_projection: true,
  projection: "Cylindrical",
  blend: "Multiband",
  bands: 0,
  crop: "Auto",
  manual_crop: [0, 0, 0, 0],
  full_sphere: false,
  scale: 1,
  max_megapixels: 300,
  yaw: 0,
  pitch: 0,
  roll: 0,
  fov: 0,
  level_pitch: 0,
  level_roll: 0,
  circular_fisheye: false,
  bicubic: true,
  flip_h: false,
  flip_v: false,
});

const defaults = (): Settings => ({
  lang: "pl",
  align: { lens: "Rectilinear", manual_hfov: null, max_features: 2500, strategy: "Auto", window: 3, optimize_focal: true, straighten: true, ransac_threshold_px: 3 },
  manualFov: false,
  fovDeg: 60,
  photo: { exposure: true, color: true, vignetting: true },
  render: { ...defaultRender(), max_megapixels: IS_MOBILE ? 60 : 300 },
  output: { format: "Auto", jpeg_quality: 95, transparent: false, gpano: true },
  ui: { showGrid: true, showOutlines: false, showNav: !IS_MOBILE, quality: IS_MOBILE ? "fast" : "normal", revealAfter: !IS_MOBILE, theme: "dark", customColor: "#1e2a44", checkUpdates: true, skippedVersion: "" },
});

export interface ImageItem {
  id: number;
  path: string;
  name: string;
  status: "loading" | "ok" | "error";
  error?: string;
  width: number;
  height: number;
  isRaw: boolean;
  summary: string;
  camera: string;
  thumb: string;
  enabled: boolean;
}

export interface CamInfo {
  id: number;
  aligned: boolean;
  f: number;
  rot: number[];
  w: number;
  h: number;
}

export interface ManualPoint {
  a: number;
  b: number;
  ax: number;
  ay: number;
  bx: number;
  by: number;
}

export interface AlignInfo {
  quality: number;
  qualityLevel: "good" | "fair" | "poor";
  issues: string[];
  pairList: { a: number; b: number; points: number; confidence: number }[];
  suggest: [number, number] | null;
  cameras: CamInfo[];
  lens: LensType;
  rms: number;
  focalSource: string;
  pairs: number;
  hfov: number;
  coverage: [number, number, number];
  autoProjection: Projection;
  seconds: number;
}

export interface PairReport {
  a: number;
  b: number;
  matches: number;
  inliers: number;
  accepted: boolean;
  error: number | null;
  status: "ok" | "highError" | "rejected" | "weak";
}

export interface MatchReport {
  failed: boolean;
  unaligned: number[];
  pairs: PairReport[];
  suggest: [number, number] | null;
}

export interface LayoutInfo {
  projection: Projection;
  u0: number;
  v0: number;
  u1: number;
  v1: number;
  outW: number;
  outH: number;
}

export type JobKind = "load" | "align" | "sphere" | "render";

interface LogEntry {
  time: Date;
  text: string;
  error: boolean;
}

export interface Toast {
  id: number;
  text: string;
  kind: "ok" | "error" | "info";
  action?: { label: string; run: () => void };
}

export const S = $state({ settings: defaults() });

export const app = $state({
  ready: false,
  images: [] as ImageItem[],
  selectedId: null as number | null,
  align: null as AlignInfo | null,
  alignedIds: [] as number[],
  sphere: null as { meta: SphereMeta; pixels: Uint8Array; version: number } | null,
  layout: null as LayoutInfo | null,
  layoutError: null as string | null,
  job: null as { kind: JobKind; stage: string; fraction: number; started: number } | null,
  sphereBuilding: false,
  log: [] as LogEntry[],
  toasts: [] as Toast[],
  dialogs: { about: false, shortcuts: false, log: false, exit: false, cp: false },
  manualPoints: [] as ManualPoint[],
  cpPair: null as [number, number] | null,
  report: null as MatchReport | null,
  alignFailed: false,
  dragOver: false,
  exportedSnapshot: null as string | null,
  workVersion: 0,
  autoCrop: null as [number, number, number, number] | null,
});

// ---------------------------------------------------------------------------- helpers

let toastId = 0;
export function toast(text: string, kind: Toast["kind"] = "info", action?: Toast["action"]) {
  const id = ++toastId;
  app.toasts.push({ id, text, kind, action });
  setTimeout(() => dismissToast(id), kind === "error" ? 9000 : 6000);
}
export function dismissToast(id: number) {
  const i = app.toasts.findIndex((x) => x.id === id);
  if (i >= 0) app.toasts.splice(i, 1);
}
function log(text: string, error = false) {
  app.log.push({ time: new Date(), text, error });
}

function deepMerge<T>(base: T, over: any): T {
  if (over === null || typeof over !== "object" || Array.isArray(over)) return (over ?? base) as T;
  const out: any = Array.isArray(base) ? [...(base as any)] : { ...(base as any) };
  for (const k of Object.keys(base as any)) {
    if (k in over) {
      const bv = (base as any)[k];
      out[k] = bv !== null && typeof bv === "object" && !Array.isArray(bv) ? deepMerge(bv, over[k]) : over[k];
    }
  }
  return out;
}

export function effectiveProjection(): Projection {
  const r = S.settings.render;
  if (r.auto_projection && app.align) return app.align.autoProjection;
  return r.projection;
}

// -------------------------------------------------------------------------- persistence

let saveTimer: any = null;
export function persist() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    invoke("save_settings", { settings: $state.snapshot(S.settings) }).catch(() => {});
  }, 400);
}

let autoAlign = false;
export async function init() {
  const saved = await invoke<any>("load_settings").catch(() => null);
  let s = defaults();
  if (saved) s = deepMerge(s, saved);
  else s.lang = ((await invoke<string>("system_language").catch(() => "pl")) as Lang) ?? "pl";
  // a new session always starts with a neutral view
  Object.assign(s.render, { yaw: 0, pitch: 0, roll: 0, flip_h: false, flip_v: false, crop: s.render.crop === "Manual" ? "Auto" : s.render.crop });
  S.settings = s;
  setTheme(s.ui.theme, s.ui.customColor, false);
  setLang(s.lang);
  await invoke("set_language", { lang: s.lang });

  await listen<{ stage: string; fraction: number }>("progress", (e) => {
    if (app.job) {
      app.job.stage = e.payload.stage;
      app.job.fraction = e.payload.fraction;
    }
  });
  await listen<any>("image-loaded", (e) => {
    const p = e.payload;
    const it = app.images.find((x) => x.id === p.id);
    if (!it) return;
    if (p.ok) Object.assign(it, { status: "ok", width: p.width, height: p.height, isRaw: p.isRaw, summary: p.summary, camera: p.camera, thumb: p.thumb });
    else {
      it.status = "error";
      it.error = p.error;
      log(`${it.name}: ${p.error}`, true);
    }
  });
  await listen("load-finished", () => {
    if (app.job?.kind === "load") app.job = null;
    const ok = app.images.filter((x) => x.status === "ok").length;
    log(`${t("loaded")}: ${ok}`);
    const bad = app.images.filter((x) => x.status === "error").length;
    if (bad) toast(`${bad} × ${t("loadFailed")}`, "error");
    if (autoAlign && !app.align) align();
  });
  autoAlign = await invoke<boolean>("auto_align_flag").catch(() => false);
  const files = await invoke<string[]>("startup_files").catch(() => []);
  if (files.length) addPaths(files);
  app.ready = true;
}

export const themeState = $state({ version: 0 });

export function setTheme(id: ThemeId, custom?: string, save = true) {
  if (IS_MOBILE && THEMES.find((x) => x.id === id)?.desktopOnly) id = "dark";
  S.settings.ui.theme = id;
  if (custom) S.settings.ui.customColor = custom;
  applyTheme(id, S.settings.ui.customColor);
  themeState.version++;
  if (save) persist();
}

export function setLanguage(l: Lang) {
  S.settings.lang = l;
  setLang(l);
  invoke("set_language", { lang: l });
  persist();
}

// ------------------------------------------------------------------------------- images

const IMAGE_EXT = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp", "arw", "srf", "sr2", "cr2", "crw", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "srw", "3fr", "erf", "kdc", "dcr", "mrw", "mos", "iiq", "mef", "rwl", "x3f"];

export function busyBlocking(): boolean {
  return !!app.job && app.job.kind !== "sphere";
}

export async function addPaths(paths: string[]) {
  if (app.job && app.job.kind !== "load" && app.job.kind !== "sphere") return;
  const added = await invoke<{ id: number; path: string; name: string }[]>("add_images", { paths });
  if (!added.length) return;
  for (const a of added) {
    app.images.push({ ...a, status: "loading", width: 0, height: 0, isRaw: false, summary: "", camera: "", thumb: "", enabled: true });
  }
  invalidateAlignment();
  app.job = { kind: "load", stage: t("stageLoading"), fraction: 0, started: Date.now() };
}

function fileName(uri: string): string {
  const s = decodeURIComponent(uri.split(/[\\/]/).pop() ?? "image");
  return s.includes(":") ? s.split(":").pop()! : s;
}

/** Mobile: content URIs are read in the UI and streamed to the engine as raw bytes. */
async function importUris(uris: string[]) {
  if (app.job && app.job.kind !== "load" && app.job.kind !== "sphere") return;
  app.job = { kind: "load", stage: t("stageLoading"), fraction: 0, started: Date.now() };
  for (const [i, uri] of uris.entries()) {
    try {
      const bytes = await readFile(uri);
      const added = await invoke<{ id: number; path: string; name: string }[]>("add_image_bytes", bytes, { headers: { "x-name": encodeURIComponent(fileName(uri)) } });
      for (const a of added) app.images.push({ ...a, status: "loading", width: 0, height: 0, isRaw: false, summary: "", camera: "", thumb: "", enabled: true });
      invalidateAlignment();
    } catch (e) {
      toast(`${fileName(uri)}: ${e}`, "error");
    }
    if (app.job) app.job.fraction = (i + 1) / uris.length;
  }
}

export async function pickFiles() {
  if (IS_MOBILE) {
    const r = await open({ multiple: true, filters: [{ name: "Images", extensions: IMAGE_EXT }] });
    if (r) importUris(Array.isArray(r) ? r : [r]);
    return;
  }
  const r = await open({ multiple: true, filters: [{ name: "Images / RAW", extensions: [...IMAGE_EXT, ...IMAGE_EXT.map((e) => e.toUpperCase())] }] });
  if (r) addPaths(Array.isArray(r) ? r : [r]);
}

export async function pickFolder() {
  const r = await open({ directory: true });
  if (r && typeof r === "string") addPaths([r]);
}

export function removeImage(id: number) {
  if (busyBlocking()) return;
  invoke("remove_images", { ids: [id] });
  app.images = app.images.filter((x) => x.id !== id);
  if (app.selectedId === id) app.selectedId = null;
  invalidateAlignment();
}

export function clearImages() {
  if (busyBlocking()) return;
  invoke("remove_images", { ids: app.images.map((x) => x.id) });
  app.images = [];
  app.selectedId = null;
  invalidateAlignment();
  app.exportedSnapshot = null;
}

export function toggleImage(id: number) {
  const it = app.images.find((x) => x.id === id);
  if (!it || busyBlocking()) return;
  it.enabled = !it.enabled;
  invalidateAlignment();
}

export function setAllEnabled(v: boolean) {
  if (busyBlocking()) return;
  app.images.forEach((x) => (x.enabled = v));
  invalidateAlignment();
}

export function sortImages() {
  app.images.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
  invalidateAlignment();
}

export function reveal(path: string) {
  revealItemInDir(path).catch(() => {});
}

export function invalidateAlignment() {
  app.alignFailed = false;
  app.report = null;
  app.align = null;
  app.alignedIds = [];
  app.sphere = null;
  app.layout = null;
  app.autoCrop = null;
  app.workVersion++;
}

export const imageName = (id: number) => app.images.find((x) => x.id === id)?.name ?? "?";

/** "a, b, c i 2 inne" – short list of image names for messages. */
export function nameList(ids: number[], max = 3): string {
  const n = ids.slice(0, max).map(imageName).join(", ");
  return ids.length > max ? `${n} ${t("andMore").replace("{n}", String(ids.length - max))}` : n;
}

/** Pairs where the user placed fewer than 2 control points – the aligner ignores those
 *  unless the pair also matched automatically. */
export function lonelyManualPairs(): [number, number][] {
  const cnt = new Map<string, number>();
  for (const m of app.manualPoints) {
    const k = m.a < m.b ? `${m.a}:${m.b}` : `${m.b}:${m.a}`;
    cnt.set(k, (cnt.get(k) ?? 0) + 1);
  }
  const out: [number, number][] = [];
  for (const [k, c] of cnt) {
    if (c >= 2) continue;
    const [a, b] = k.split(":").map(Number);
    const auto = app.report?.pairs.some((p) => p.accepted && ((p.a === a && p.b === b) || (p.a === b && p.b === a)));
    if (!auto) out.push([a, b]);
  }
  return out;
}

async function loadReport(ids: number[]) {
  app.report = await invoke<MatchReport>("match_report").catch(() => null);
  app.cpPair = app.report?.suggest ?? app.align?.suggest ?? (ids.length >= 2 ? [ids[0], ids[1]] : null);
}

export function readyIds(): number[] {
  return app.images.filter((x) => x.enabled && x.status === "ok").map((x) => x.id);
}

// ------------------------------------------------------------------------------- align

export async function align() {
  if (busyBlocking()) return;
  const ids = readyIds();
  if (ids.length < 2) {
    toast(t("needTwo"), "error");
    return;
  }
  if (app.job?.kind === "sphere") invoke("cancel_job");
  const st = $state.snapshot(S.settings);
  const settings = { ...st.align, manual_hfov: st.manualFov ? st.fovDeg : null };
  app.job = { kind: "align", stage: "", fraction: 0, started: Date.now() };
  try {
    const manual = app.manualPoints.filter((m) => ids.includes(m.a) && ids.includes(m.b));
    const info = await invoke<AlignInfo>("align", { ids, settings, photo: st.photo, manual });
    app.alignFailed = false;
    app.align = info;
    await loadReport(ids);
    app.alignedIds = ids;
    app.workVersion++;
    const n = info.cameras.filter((c) => c.aligned).length;
    log(`${t("alignDone")}: ${n}/${ids.length} · RMS ${info.rms.toFixed(2)} px · ${info.seconds.toFixed(1)} s`);
    const missing = app.report?.unaligned ?? info.cameras.filter((c) => !c.aligned).map((c) => c.id);
    const openCp = { label: t("cpOpen"), run: () => (app.dialogs.cp = true) };
    if (missing.length) {
      log(`${t("qNotConnected")}: ${nameList(missing, 50)}`, true);
      toast(`${n}/${ids.length} ${t("aligned")} · ${t("qNotConnected")}: ${nameList(missing)}`, "error", openCp);
    } else if (info.qualityLevel !== "good") {
      toast(`${t("qualityLow")} (${info.quality}/100)`, "error", openCp);
    } else toast(`${t("alignDone")}: ${n}/${ids.length} · RMS ${info.rms.toFixed(2)} px · ${info.quality}/100`, "ok");
    const lonely = lonelyManualPairs().filter(([a, b]) => ids.includes(a) && ids.includes(b));
    if (lonely.length) toast(`${t("cpLonely")}: ${lonely.map(([a, b]) => `${imageName(a)} ↔ ${imageName(b)}`).join(", ")}`, "info", openCp);
    const r = S.settings.render;
    Object.assign(r, { yaw: 0, pitch: 0, roll: 0, level_pitch: 0, level_roll: 0, flip_h: false, flip_v: false });
    if (r.crop === "Manual") r.crop = "Auto";
    resetHistory();
    app.job = null;
    await buildSphere();
  } catch (e: any) {
    app.job = null;
    const msg = String(e);
    log(`${t("alignFailed")}: ${msg}`, true);
    if (!msg.toLowerCase().includes("anulowano") && !msg.toLowerCase().includes("cancel")) {
      app.alignFailed = true;
      await loadReport(ids);
      toast(`${t("alignFailed")}: ${msg}`, "error", { label: t("cpOpen"), run: () => (app.dialogs.cp = true) });
    }
  }
}

const QUALITY_MP = { fast: 3, normal: 7, high: 14 };

let sphereVersion = 0;
export async function buildSphere() {
  if (!app.align) return;
  if (app.job && app.job.kind !== "sphere") return;
  if (app.job?.kind === "sphere") {
    await invoke("cancel_job");
  }
  const myVersion = ++sphereVersion;
  app.job = { kind: "sphere", stage: t("buildingPreview"), fraction: 0, started: Date.now() };
  app.sphereBuilding = true;
  try {
    const meta = await invoke<SphereMeta>("build_sphere", { render: $state.snapshot(S.settings.render), megapixels: QUALITY_MP[S.settings.ui.quality] });
    if (myVersion !== sphereVersion) return;
    const buf = await invoke<ArrayBuffer>("sphere_pixels");
    if (myVersion !== sphereVersion) return;
    app.sphere = { meta, pixels: new Uint8Array(buf), version: myVersion };
    await refreshLayout();
  } catch (e: any) {
    if (myVersion === sphereVersion && !String(e).toLowerCase().includes("cancel") && !String(e).includes("Anulowano")) {
      toast(`${t("previewFailed")}: ${e}`, "error");
    }
  } finally {
    if (myVersion === sphereVersion) {
      app.sphereBuilding = false;
      if (app.job?.kind === "sphere") app.job = null;
    }
  }
}

export async function setPhotometric() {
  persist();
  if (!app.align) return;
  await invoke("set_photometric", { photo: $state.snapshot(S.settings.photo) }).catch(() => {});
  app.workVersion++;
  buildSphere();
}

let layoutSeq = 0;
export async function refreshLayout() {
  if (!app.align) return;
  const seq = ++layoutSeq;
  try {
    const l = await invoke<LayoutInfo>("layout", { render: $state.snapshot(S.settings.render) });
    if (seq !== layoutSeq) return;
    app.layout = l;
    app.layoutError = null;
  } catch (e: any) {
    if (seq !== layoutSeq) return;
    app.layout = null;
    app.layoutError = String(e);
  }
}

// ------------------------------------------------------------------------------ render

export function anyRaw(): boolean {
  const ids = app.alignedIds.length ? app.alignedIds : readyIds();
  return app.images.some((x) => ids.includes(x.id) && x.isRaw);
}

export function resolvedFormat(): Exclude<OutFormat, "Auto"> {
  const f = S.settings.output.format;
  if (f === "Auto") return anyRaw() ? "Dng" : "Jpeg";
  return f;
}

const EXT: Record<string, string> = { Jpeg: "jpg", Tiff16: "tif", Png16: "png", Dng: "dng" };

export function viewSnapshot(): string {
  return JSON.stringify({ r: $state.snapshot(S.settings.render), w: app.workVersion });
}

export function hasUnsavedWork(): boolean {
  if (!app.images.length) return false;
  if (app.job?.kind === "render") return true;
  return app.exportedSnapshot !== viewSnapshot();
}

export async function renderAndSave() {
  if (!app.align || busyBlocking()) return;
  const fmt = resolvedFormat();
  const first = app.images.find((x) => x.id === app.alignedIds[0]);
  const base = first ? first.name.replace(/\.[^.]+$/, "") + "_pano" : "panorama";
  const dir = first ? first.path.replace(/[\\/][^\\/]*$/, "") : undefined;
  if (IS_MOBILE) return renderMobile(base, fmt);
  const path = await save({
    defaultPath: dir ? `${dir}\\${base}.${EXT[fmt]}` : `${base}.${EXT[fmt]}`,
    filters: [{ name: fmt.replace("16", ""), extensions: fmt === "Jpeg" ? ["jpg", "jpeg"] : fmt === "Tiff16" ? ["tif", "tiff"] : [EXT[fmt]] }],
  });
  if (!path) return;
  if (app.job?.kind === "sphere") await invoke("cancel_job");
  const render = $state.snapshot(S.settings.render);
  const output = { ...$state.snapshot(S.settings.output), format: fmt };
  const snap = viewSnapshot();
  app.job = { kind: "render", stage: "", fraction: 0, started: Date.now() };
  try {
    const r = await invoke<{ path: string; format: string; width: number; height: number; seconds: number }>("render_and_save", { path, render, output });
    app.exportedSnapshot = snap;
    const msg = `${t("saved")}: ${r.path.split(/[\\/]/).pop()} · ${r.width}×${r.height} (${((r.width * r.height) / 1e6).toFixed(1)} MP) · ${r.seconds.toFixed(1)} s`;
    log(msg);
    toast(msg, "ok", { label: t("showFile"), run: () => reveal(r.path) });
    if (S.settings.ui.revealAfter) reveal(r.path);
  } catch (e: any) {
    log(`${t("renderFailed")}: ${e}`, true);
    toast(`${t("renderFailed")}: ${e}`, "error");
  } finally {
    app.job = null;
  }
}

/** Mobile: render into the cache, then copy to a location chosen in the system dialog. */
async function renderMobile(base: string, fmt: Exclude<OutFormat, "Auto">) {
  if (app.job?.kind === "sphere") await invoke("cancel_job");
  const cache = await invoke<string>("cache_dir");
  const tmp = `${cache}/${base}.${EXT[fmt]}`;
  const render = $state.snapshot(S.settings.render);
  const output = { ...$state.snapshot(S.settings.output), format: fmt };
  const snap = viewSnapshot();
  app.job = { kind: "render", stage: "", fraction: 0, started: Date.now() };
  try {
    const r = await invoke<{ path: string; width: number; height: number; seconds: number }>("render_and_save", { path: tmp, render, output });
    app.job = null;
    const dest = await save({ defaultPath: `${base}.${EXT[fmt]}`, filters: [{ name: fmt, extensions: [EXT[fmt]] }] }).catch(() => null);
    if (dest) {
      const buf = await invoke<ArrayBuffer>("read_file_bytes", { path: tmp });
      await writeFile(dest, new Uint8Array(buf));
      app.exportedSnapshot = snap;
      toast(`${t("saved")}: ${r.width}×${r.height} · ${r.seconds.toFixed(1)} s`, "ok");
    } else {
      toast(`${t("saved")}: ${tmp}`, "info");
    }
  } catch (e: any) {
    toast(`${t("renderFailed")}: ${e}`, "error");
  } finally {
    app.job = null;
  }
}

export function cancelJob() {
  invoke("cancel_job");
}

// ------------------------------------------------------------------------------ history

type ViewState = Pick<
  RenderSettings,
  "auto_projection" | "projection" | "full_sphere" | "yaw" | "pitch" | "roll" | "fov" | "level_pitch" | "level_roll" | "flip_h" | "flip_v" | "crop" | "manual_crop"
>;
const H = $state({ stack: [] as string[], pos: -1 });

function currentView(): string {
  const r = S.settings.render;
  const v: ViewState = {
    auto_projection: r.auto_projection,
    projection: r.projection,
    full_sphere: r.full_sphere,
    yaw: +r.yaw.toFixed(3),
    pitch: +r.pitch.toFixed(3),
    roll: +r.roll.toFixed(3),
    fov: r.fov,
    level_pitch: +r.level_pitch.toFixed(3),
    level_roll: +r.level_roll.toFixed(3),
    flip_h: r.flip_h,
    flip_v: r.flip_v,
    crop: r.crop,
    manual_crop: [...r.manual_crop] as any,
  };
  return JSON.stringify(v);
}

export function resetHistory() {
  H.stack = [currentView()];
  H.pos = 0;
}

/** Record the current view as a history step (call after an interaction completes). */
export function commitHistory() {
  const cur = currentView();
  if (H.stack[H.pos] === cur) return;
  H.stack = H.stack.slice(0, H.pos + 1);
  H.stack.push(cur);
  if (H.stack.length > 200) H.stack.shift();
  H.pos = H.stack.length - 1;
}

function applyView(s: string) {
  Object.assign(S.settings.render, JSON.parse(s));
  refreshLayout();
}

export function canUndo() {
  return H.pos > 0 || (H.stack.length > 0 && H.stack[H.pos] !== currentView());
}
export function canRedo() {
  return H.pos < H.stack.length - 1;
}
export function undo() {
  commitHistory();
  if (H.pos > 0) {
    H.pos--;
    applyView(H.stack[H.pos]);
  }
}
export function redo() {
  if (H.pos < H.stack.length - 1) {
    H.pos++;
    applyView(H.stack[H.pos]);
  }
}

/** Called after any discrete change of view-related settings. */
export function viewChanged() {
  syncLevel();
  refreshLayout();
  persist();
  clearTimeout(histTimer);
  histTimer = setTimeout(commitHistory, 350);
}
let histTimer: any = null;

// ------------------------------------------------------------------------- viewer state

export type Tool = "rotate" | "pan" | "center" | "horizon" | "vertical" | "crop";
export const vw = $state({
  tool: "rotate" as Tool,
  zoom: 1,
  panX: 0,
  panY: 0,
  interacting: false,
  points: 0,
});

export function fitView() {
  vw.zoom = 1;
  vw.panX = 0;
  vw.panY = 0;
}

export function nudgeView(dyaw: number, dpitch: number, droll: number) {
  const r = S.settings.render;
  r.yaw = ((((r.yaw + dyaw + 180) % 360) + 360) % 360) - 180;
  // in strip panoramas tilting only bends the horizon – up/down does nothing there
  if (currentKind() !== "strip") r.pitch = Math.max(-90, Math.min(90, r.pitch + dpitch));
  r.roll = ((((r.roll + droll + 180) % 360) + 360) % 360) - 180;
}

export function resetView() {
  Object.assign(S.settings.render, { yaw: 0, pitch: 0, roll: 0, level_pitch: 0, level_roll: 0, fov: 0, flip_h: false, flip_v: false });
  viewChanged();
}

// --------------------------------------------------------------------------- composition

export function currentKind(): Kind {
  return kindOf(effectiveProjection());
}

/** In strip panoramas pitch/roll only ever level the horizon – remember them as the level. */
function syncLevel() {
  const r = S.settings.render;
  if (currentKind() === "strip") {
    r.level_pitch = r.pitch;
    r.level_roll = r.roll;
  }
}

/** Choose the projection (or "auto"); orientation is carried over in a way that fits the new type. */
export function chooseProjection(p: Projection | "auto") {
  const r = S.settings.render;
  const before = currentKind();
  if (p === "auto") r.auto_projection = true;
  else {
    r.auto_projection = false;
    r.projection = p;
  }
  const after = currentKind();
  if (before !== after) {
    // drop look-up/down and frame rotation offsets: start from a level view in the same direction
    r.pitch = r.level_pitch;
    r.roll = r.level_roll;
    r.fov = 0;
  }
  viewChanged();
}

/** Look offsets of view-type projections, relative to the level horizon. */
export function lookOffsets(): { pitch: number; roll: number } {
  const r = S.settings.render;
  return { pitch: r.pitch - r.level_pitch, roll: wrap180(r.roll - r.level_roll) };
}
export function setLookOffsets(pitch: number | null, roll: number | null) {
  const r = S.settings.render;
  if (pitch != null) r.pitch = Math.max(-90, Math.min(90, r.level_pitch + pitch));
  if (roll != null) r.roll = wrap180(r.level_roll + roll);
}

/** Apply a new total rotation produced by the horizon / vertical tools: it becomes the new level,
 *  and view-type projections keep looking the same way relative to it. */
export function applyLevel(yaw: number, pitch: number, roll: number) {
  const r = S.settings.render;
  const off = lookOffsets();
  r.yaw = yaw;
  r.pitch = pitch;
  r.roll = roll;
  if (currentKind() !== "strip") {
    r.level_pitch = pitch - off.pitch;
    r.level_roll = wrap180(roll - off.roll);
  }
  viewChanged();
}

/** Centre the panorama on a world direction: strips only turn (keeping the horizon level). */
export function centreOn(yaw: number, pitch: number) {
  const r = S.settings.render;
  r.yaw = yaw;
  if (currentKind() !== "strip") r.pitch = pitch;
  viewChanged();
}
