// Update check against the release feed on redzikowski.com (GitHub-shaped, the repo is private)
// – works the same on desktop, Android and iOS; plain fetch from the webview, the feed allows CORS.
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

const API = "https://redzikowski.com/api/github/repos/BlackSingularities/OpenPano/releases/latest";
const RECHECK_MS = 6 * 3600 * 1000;

export interface UpdateInfo {
  version: string;
  page: string;
  download: string | null;
}

export const upd = $state({
  current: "",
  available: null as UpdateInfo | null,
  checking: false,
  lastError: false,
  lastChecked: 0,
});

function parse(v: string): number[] {
  return v.replace(/^v/i, "").split(/[.+-]/).slice(0, 3).map((x) => parseInt(x, 10) || 0);
}
export function newer(a: string, b: string): boolean {
  const x = parse(a), y = parse(b);
  for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  return false;
}

async function assetPattern(): Promise<RegExp | null> {
  const ua = navigator.userAgent;
  if (/android/i.test(ua)) return /android\.apk$/i;
  if (/iphone|ipad|ipod/i.test(ua)) return null; // sideloaded IPA – release page is the right place
  if (/windows/i.test(ua)) return (await invoke<boolean>("portable_build").catch(() => false)) ? /portable-x64\.exe$/i : /setup-x64\.exe$/i;
  return null;
}

/** Returns true when the check itself succeeded. */
export async function checkForUpdate(): Promise<boolean> {
  if (upd.checking) return false;
  upd.checking = true;
  try {
    if (!upd.current) upd.current = await getVersion();
    const r = await fetch(API, { headers: { Accept: "application/vnd.github+json" }, cache: "no-store" });
    if (!r.ok) throw new Error(String(r.status));
    const rel = await r.json();
    if (rel.draft || rel.prerelease) return true;
    const version = String(rel.tag_name ?? "").replace(/^v/i, "");
    if (version && newer(version, upd.current)) {
      const pat = await assetPattern();
      const asset = pat ? (rel.assets ?? []).find((a: any) => pat.test(a.name)) : null;
      upd.available = { version, page: rel.html_url, download: asset?.browser_download_url ?? null };
    } else upd.available = null;
    upd.lastError = false;
    return true;
  } catch {
    upd.lastError = true;
    return false;
  } finally {
    upd.lastChecked = Date.now();
    upd.checking = false;
  }
}

let started = false;
/** Checks shortly after start-up and then periodically while the app stays open. */
export function startUpdateChecks(enabled: () => boolean) {
  if (started) return;
  started = true;
  getVersion().then((v) => (upd.current = v)).catch(() => {});
  const run = () => enabled() && checkForUpdate();
  setTimeout(run, 4000);
  setInterval(run, RECHECK_MS);
  // mobile apps are resumed rather than restarted – re-check when coming back
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && Date.now() - upd.lastChecked > RECHECK_MS) run();
  });
}

export function openUpdate(direct: boolean) {
  const u = upd.available;
  if (u) openUrl(direct && u.download ? u.download : u.page);
}
