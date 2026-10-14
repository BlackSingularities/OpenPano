<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./components/Icon.svelte";
  import ImageList from "./components/ImageList.svelte";
  import Inspector from "./components/Inspector.svelte";
  import Viewer from "./components/Viewer.svelte";
  import Tooltip from "./components/Tooltip.svelte";
  import { t, getLang } from "./lib/i18n.svelte";
  import {
    S, app, vw, init, setLanguage, addPaths, pickFiles, pickFolder, align, renderAndSave, cancelJob, busyBlocking, readyIds,
    hasUnsavedWork, undo, redo, fitView, nudgeView, viewChanged, resetView, persist, dismissToast, buildSphere,
  } from "./lib/store.svelte";

  const AUTHOR = "https://redzikowski.com";
  const REPO = "https://github.com/BlackSingularities/Panoramer";
  const VERSION = "2.0.0";

  let menuOpen = $state(false);
  let now = $state(Date.now());
  let allowClose = false;

  const step = $derived(!app.images.length ? 1 : !app.align ? 2 : 3);
  const unsaved = $derived(hasUnsavedWork());

  onMount(() => {
    init();
    const tick = setInterval(() => (now = Date.now()), 500);
    const unsubs: (() => void)[] = [];
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const p: any = e.payload;
        if (p.type === "enter" || p.type === "over") app.dragOver = true;
        else if (p.type === "leave") app.dragOver = false;
        else if (p.type === "drop") {
          app.dragOver = false;
          if (p.paths?.length) addPaths(p.paths);
        }
      })
      .then((u) => unsubs.push(u));
    getCurrentWindow()
      .onCloseRequested((e) => {
        if (!allowClose && hasUnsavedWork()) {
          e.preventDefault();
          app.dialogs.exit = true;
        }
      })
      .then((u) => unsubs.push(u));
    return () => {
      clearInterval(tick);
      unsubs.forEach((u) => u());
    };
  });

  async function quitAnyway() {
    allowClose = true;
    if (app.job) cancelJob();
    await getCurrentWindow().destroy();
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (ctrl && k === "z" && !e.shiftKey) return (e.preventDefault(), undo());
    if (ctrl && (k === "y" || (k === "z" && e.shiftKey))) return (e.preventDefault(), redo());
    if (ctrl && k === "o") return (e.preventDefault(), pickFiles());
    if (ctrl && k === "enter") return (e.preventDefault(), align());
    if (ctrl && k === "s") return (e.preventDefault(), renderAndSave());
    if (ctrl || e.altKey) return;
    if (app.dialogs.exit || app.dialogs.about || app.dialogs.shortcuts || app.dialogs.log) {
      if (k === "escape") Object.keys(app.dialogs).forEach((d) => ((app.dialogs as any)[d] = false));
      return;
    }
    const st = e.shiftKey ? 10 : 1;
    const view = (dy: number, dp: number, dr: number) => {
      e.preventDefault();
      nudgeView(dy, dp, dr);
      viewChanged();
    };
    switch (k) {
      case "arrowleft":
        return view(-st, 0, 0);
      case "arrowright":
        return view(st, 0, 0);
      case "arrowup":
        return view(0, st, 0);
      case "arrowdown":
        return view(0, -st, 0);
      case "q":
        return view(0, 0, st);
      case "e":
        return view(0, 0, -st);
      case "f":
        return fitView();
      case "g":
        S.settings.ui.showGrid = !S.settings.ui.showGrid;
        return persist();
      case "o":
        S.settings.ui.showOutlines = !S.settings.ui.showOutlines;
        return persist();
      case "r":
        return (vw.tool = "rotate");
      case "h":
        return (vw.tool = "pan");
      case "c":
        return (vw.tool = "center");
      case "l":
        return (vw.tool = "horizon");
      case "v":
        return (vw.tool = "vertical");
      case "k":
        return (vw.tool = "crop");
      case "0":
        return resetView();
    }
  }

  const shortcuts = $derived([
    ["Ctrl+O", t("scOpen")],
    ["Ctrl+Enter", t("scAlign")],
    ["Ctrl+S", t("scRender")],
    ["Ctrl+Z / Ctrl+Y", t("scUndo")],
    ["← → ↑ ↓", t("scArrows")],
    ["Q / E", t("scRoll")],
    ["R · H · C · L · V · K", t("scTools")],
    ["F", t("scFit")],
    ["G / O", t("scOverlays")],
    ["0", t("scReset")],
    [t("scWheel"), t("scZoom")],
    [t("scMiddle"), t("toolPan")],
    [t("scRight") + " / Shift", t("scRoll")],
  ]);

  function setQuality(q: "fast" | "normal" | "high") {
    S.settings.ui.quality = q;
    persist();
    buildSphere();
  }
</script>

<svelte:window onkeydown={onKey} onclick={() => (menuOpen = false)} />

<Tooltip />

{#if app.ready}
  <div class="shell">
    <header>
      <div class="brand">
        <img src="/icon.png" alt="" />
        <div>
          <div class="bname">Panoramer</div>
          <div class="btag">{t("appTagline")}</div>
        </div>
      </div>

      <nav class="steps">
        {#each [t("stepImages"), t("stepAlign"), t("stepExport")] as label, i}
          <div class="step" class:done={step > i + 1} class:cur={step === i + 1}>
            <span class="dot">{#if step > i + 1}<Icon name="check" size={12} stroke={3} />{:else}{i + 1}{/if}</span>
            {label}
          </div>
          {#if i < 2}<span class="line" class:done={step > i + 1}></span>{/if}
        {/each}
      </nav>

      <div class="actions">
        <button class="btn" onclick={pickFiles} data-tip="Ctrl+O"><Icon name="plus" size={16} />{t("addImages")}</button>
        <button class="btn blue" disabled={busyBlocking() || readyIds().length < 2} onclick={align} data-tip="Ctrl+Enter"><Icon name="align" size={16} />{t("align")}</button>
        <button class="btn primary" disabled={!app.align || busyBlocking()} onclick={renderAndSave} data-tip="Ctrl+S"><Icon name="download" size={16} />{t("export")}</button>
        <div class="sep"></div>
        <div class="seg lang">
          <button class:on={getLang() === "pl"} onclick={() => setLanguage("pl")}>PL</button>
          <button class:on={getLang() === "en"} onclick={() => setLanguage("en")}>EN</button>
        </div>
        <div class="menuwrap">
          <button
            class="btn ghost icon"
            data-tip={t("menu")}
            onclick={(e) => {
              e.stopPropagation();
              menuOpen = !menuOpen;
            }}><Icon name="more" size={18} /></button
          >
          {#if menuOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="dropdown" onclick={(e) => e.stopPropagation()}>
              <button onclick={() => ((menuOpen = false), pickFolder())}><Icon name="folder" size={15} />{t("addFolder")}</button>
              <div class="dsep"></div>
              <div class="dlabel">{t("previewQuality")}</div>
              <div class="seg full q">
                <button class:on={S.settings.ui.quality === "fast"} onclick={() => setQuality("fast")}>{t("qFast")}</button>
                <button class:on={S.settings.ui.quality === "normal"} onclick={() => setQuality("normal")}>{t("qNormal")}</button>
                <button class:on={S.settings.ui.quality === "high"} onclick={() => setQuality("high")}>{t("qHigh")}</button>
              </div>
              <div class="dsep"></div>
              <button onclick={() => ((menuOpen = false), (app.dialogs.shortcuts = true))}><Icon name="keyboard" size={15} />{t("shortcuts")}</button>
              <button onclick={() => ((menuOpen = false), (app.dialogs.log = true))}><Icon name="list" size={15} />{t("log")}</button>
              <button onclick={() => ((menuOpen = false), openUrl(REPO))}><Icon name="github" size={15} />{t("projectPage")}</button>
              <button onclick={() => ((menuOpen = false), (app.dialogs.about = true))}><Icon name="info" size={15} />{t("about")}</button>
            </div>
          {/if}
        </div>
      </div>
    </header>

    <main>
      <div class="left"><ImageList /></div>
      <div class="center">
        {#if app.align}
          <Viewer />
        {:else}
          <div class="welcome">
            <div class="wcard">
              <img src="/icon.png" alt="" class="wlogo" />
              <h1>{t("welcomeTitle")}</h1>
              <p>{app.images.length ? t("welcomeAlign") : t("welcomeText")}</p>
              <div class="wbtns">
                {#if !app.images.length}
                  <button class="btn primary lg" onclick={pickFiles}><Icon name="plus" size={18} />{t("addImages")}</button>
                  <button class="btn lg" onclick={pickFolder}><Icon name="folder" size={18} />{t("addFolder")}</button>
                {:else}
                  <button class="btn blue lg" disabled={busyBlocking() || readyIds().length < 2} onclick={align}><Icon name="align" size={18} />{t("alignImages")}</button>
                  <button class="btn lg" onclick={pickFiles}><Icon name="plus" size={18} />{t("addImages")}</button>
                {/if}
              </div>
              {#if app.job?.kind === "align"}
                <div class="wprog">
                  <div class="bar"><div class="fill" style="width:{app.job.fraction * 100}%"></div></div>
                  <span>{app.job.stage}</span>
                </div>
              {/if}
              <div class="wformats">{t("formats")}</div>
            </div>
          </div>
        {/if}
      </div>
      <div class="right"><Inspector /></div>
    </main>

    <footer>
      {#if app.job && app.job.kind !== "sphere"}
        <span class="spin"></span>
        <div class="pbar"><div class="pfill" style="width:{Math.max(2, app.job.fraction * 100)}%"></div></div>
        <span class="pct">{Math.round(app.job.fraction * 100)}%</span>
        <span class="stage">{app.job.stage}</span>
        <span class="muted">{Math.round((now - app.job.started) / 1000)} s</span>
        <button class="btn sm" onclick={cancelJob}><Icon name="stop" size={12} />{t("cancel")}</button>
      {:else}
        <button class="lastlog" class:err={app.log.at(-1)?.error} onclick={() => (app.dialogs.log = true)}>
          {app.log.at(-1)?.text ?? t("ready")}
        </button>
      {/if}
      <span class="grow"></span>
      {#if unsaved && app.align}<span class="unsaved"><span class="udot"></span>{t("notExported")}</span>{/if}
      <span class="muted">Panoramer {VERSION}</span>
      <button class="link" onclick={() => openUrl(AUTHOR)}>redzikowski.com</button>
    </footer>
  </div>

  {#if app.dragOver}
    <div class="drop"><div class="dropbox"><Icon name="image" size={42} stroke={1.4} />{t("dropHere")}</div></div>
  {/if}

  <div class="toasts">
    {#each app.toasts as ts (ts.id)}
      <div class="toast {ts.kind}">
        <Icon name={ts.kind === "ok" ? "ok" : ts.kind === "error" ? "alert" : "info"} size={17} />
        <span class="tt">{ts.text}</span>
        {#if ts.action}<button class="btn sm" onclick={() => (ts.action!.run(), dismissToast(ts.id))}>{ts.action.label}</button>{/if}
        <button class="tx" onclick={() => dismissToast(ts.id)}><Icon name="x" size={14} /></button>
      </div>
    {/each}
  </div>

  {#if app.dialogs.about}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-bg" onclick={() => (app.dialogs.about = false)}>
      <div class="modal about" onclick={(e) => e.stopPropagation()}>
        <img src="/icon.png" alt="" class="alogo" />
        <h2>Panoramer <span>{VERSION}</span></h2>
        <div class="credit">
          <span>{t("ideaBy")}</span>
          <button class="link big" onclick={() => openUrl(AUTHOR)}>redzikowski.com</button>
        </div>
        <p>{t("aboutText")}</p>
        <div class="arow">
          <button class="btn sm" onclick={() => openUrl(REPO)}><Icon name="github" size={14} />GitHub</button>
          <span class="muted small">{t("license")}</span>
        </div>
        <button class="btn" onclick={() => (app.dialogs.about = false)}>{t("close")}</button>
      </div>
    </div>
  {/if}

  {#if app.dialogs.shortcuts}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-bg" onclick={() => (app.dialogs.shortcuts = false)}>
      <div class="modal" onclick={(e) => e.stopPropagation()}>
        <h3><Icon name="keyboard" size={18} />{t("shortcuts")}</h3>
        <div class="sc">
          {#each shortcuts as [k, d]}
            <span class="kbd">{k}</span><span>{d}</span>
          {/each}
        </div>
        <div class="mfoot"><button class="btn" onclick={() => (app.dialogs.shortcuts = false)}>{t("close")}</button></div>
      </div>
    </div>
  {/if}

  {#if app.dialogs.log}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-bg" onclick={() => (app.dialogs.log = false)}>
      <div class="modal wide" onclick={(e) => e.stopPropagation()}>
        <h3><Icon name="list" size={18} />{t("log")}</h3>
        <div class="log">
          {#each app.log as l}
            <div class:err={l.error}><span class="muted">{l.time.toLocaleTimeString()}</span> {l.text}</div>
          {/each}
        </div>
        <div class="mfoot">
          <button class="btn ghost" onclick={() => (app.log = [])}>{t("clearLog")}</button>
          <button class="btn" onclick={() => (app.dialogs.log = false)}>{t("close")}</button>
        </div>
      </div>
    </div>
  {/if}

  {#if app.dialogs.exit}
    <div class="modal-bg strong">
      <div class="modal exit">
        <div class="ehead"><span class="eicon"><Icon name="alert" size={22} /></span><h3>{t("exitTitle")}</h3></div>
        <p>{t("exitText")}</p>
        {#if app.job?.kind === "render"}<p class="warnp">{t("exitRendering")}</p>{/if}
        <div class="mfoot">
          <button class="btn danger" onclick={quitAnyway}>{t("exitDiscard")}</button>
          <span class="grow"></span>
          <button class="btn" onclick={() => (app.dialogs.exit = false)}>{t("exitStay")}</button>
          {#if app.align && !busyBlocking()}
            <button class="btn primary" onclick={() => ((app.dialogs.exit = false), renderAndSave())}><Icon name="download" size={15} />{t("renderSave")}</button>
          {/if}
        </div>
      </div>
    </div>
  {/if}
{/if}

<style>
  .shell {
    display: grid;
    grid-template-rows: 56px 1fr 30px;
    height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 12px 0 14px;
    background: linear-gradient(180deg, #1d212a, #181b22);
    border-bottom: 1px solid var(--line);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 200px;
  }
  .brand img {
    width: 32px;
    height: 32px;
  }
  .bname {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: 16px;
    letter-spacing: 0.2px;
  }
  .btag {
    font-size: 11px;
    color: var(--text-3);
  }
  .steps {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-3);
    font-weight: 500;
    white-space: nowrap;
  }
  .dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 700;
    background: var(--raised);
    border: 1px solid var(--line-2);
  }
  .step.cur {
    color: var(--text);
  }
  .step.cur .dot {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-ink);
    box-shadow: 0 0 0 4px var(--accent-soft);
  }
  .step.done {
    color: var(--text-2);
  }
  .step.done .dot {
    background: rgba(95, 208, 138, 0.18);
    border-color: rgba(95, 208, 138, 0.5);
    color: var(--ok);
  }
  .line {
    width: 34px;
    height: 2px;
    border-radius: 2px;
    background: var(--line-2);
  }
  .line.done {
    background: rgba(95, 208, 138, 0.6);
  }
  @media (max-width: 1350px) {
    .steps {
      visibility: hidden;
    }
    .btag {
      display: none;
    }
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sep {
    width: 1px;
    height: 24px;
    background: var(--line-2);
    margin: 0 2px;
  }
  .lang button {
    font-weight: 600;
    font-size: 11.5px;
    padding: 0 9px;
  }
  .menuwrap {
    position: relative;
  }
  .dropdown {
    position: absolute;
    right: 0;
    top: 40px;
    z-index: 60;
    width: 250px;
    padding: 6px;
    border-radius: 12px;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    box-shadow: var(--shadow);
  }
  .dropdown > button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 9px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    cursor: pointer;
    text-align: left;
  }
  .dropdown > button:hover {
    background: var(--raised-2);
  }
  .dsep {
    height: 1px;
    background: var(--line);
    margin: 5px 2px;
  }
  .dlabel {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-3);
    padding: 4px 9px 6px;
  }
  .q {
    margin: 0 4px 4px;
  }
  main {
    display: grid;
    grid-template-columns: 300px 1fr 360px;
    min-height: 0;
  }
  .left {
    min-height: 0;
    box-shadow: 1px 0 0 var(--line);
    z-index: 1;
  }
  .right {
    min-height: 0;
    box-shadow: -1px 0 0 var(--line);
    z-index: 1;
  }
  .center {
    min-width: 0;
    min-height: 0;
    position: relative;
  }
  .welcome {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(ellipse at 50% 40%, #1c2029 0%, #111318 70%);
  }
  .wcard {
    width: min(540px, 90%);
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }
  .wlogo {
    width: 84px;
    height: 84px;
    filter: drop-shadow(0 10px 30px rgba(255, 170, 60, 0.18));
  }
  .wcard h1 {
    margin: 4px 0 0;
    font-family: var(--font-display);
    font-size: 28px;
    font-weight: 700;
  }
  .wcard p {
    margin: 0;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.5;
  }
  .wbtns {
    display: flex;
    gap: 10px;
    margin-top: 10px;
  }
  .wprog {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: var(--text-2);
    margin-top: 6px;
  }
  .bar {
    height: 6px;
    border-radius: 6px;
    background: var(--raised);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--blue), var(--accent));
    transition: width 0.2s;
  }
  .wformats {
    margin-top: 16px;
    font-size: 11.5px;
    color: var(--text-3);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px;
    background: var(--bg-2);
    box-shadow: 0 -1px 0 var(--line);
    font-size: 12px;
    min-width: 0;
  }
  .grow {
    flex: 1;
  }
  .pbar {
    width: 220px;
    height: 6px;
    border-radius: 6px;
    background: var(--raised);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    background: linear-gradient(90deg, var(--blue), var(--accent));
    transition: width 0.15s;
  }
  .pct {
    font-variant-numeric: tabular-nums;
    min-width: 34px;
  }
  .stage {
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .spin {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .lastlog {
    border: 0;
    background: none;
    color: var(--text-3);
    cursor: pointer;
    padding: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 60vw;
    font-size: 12px;
  }
  .lastlog.err {
    color: var(--err);
  }
  .unsaved {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent-2);
  }
  .udot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
  .link.big {
    font-size: 15px;
    font-weight: 600;
  }
  .drop {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: rgba(8, 9, 12, 0.72);
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }
  .dropbox {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 50px 80px;
    border-radius: 18px;
    border: 2px dashed var(--accent);
    background: var(--accent-soft);
    color: var(--accent-2);
    font-size: 18px;
    font-weight: 600;
  }
  .toasts {
    position: fixed;
    bottom: 42px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 90;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px 9px 13px;
    border-radius: 12px;
    background: rgba(32, 36, 46, 0.97);
    border: 1px solid var(--line-2);
    box-shadow: var(--shadow);
    max-width: 720px;
    animation: up 0.18s ease-out;
  }
  .toast.ok :global(svg:first-child) {
    color: var(--ok);
  }
  .toast.error {
    border-color: rgba(255, 107, 107, 0.45);
  }
  .toast.error :global(svg:first-child) {
    color: var(--err);
  }
  .toast.info :global(svg:first-child) {
    color: var(--blue);
  }
  .tt {
    line-height: 1.35;
  }
  .tx {
    border: 0;
    background: none;
    color: var(--text-3);
    cursor: pointer;
    display: flex;
    padding: 2px;
  }
  @keyframes up {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
  .modal-bg {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: rgba(5, 6, 9, 0.6);
    backdrop-filter: blur(3px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .modal-bg.strong {
    background: rgba(5, 6, 9, 0.75);
  }
  .modal {
    width: 480px;
    max-width: 92vw;
    padding: 20px 22px;
    border-radius: 16px;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6);
    animation: pop 0.16s ease-out;
  }
  .modal.wide {
    width: 680px;
  }
  @keyframes pop {
    from {
      transform: scale(0.97);
      opacity: 0;
    }
  }
  .modal h3 {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 0 0 14px;
    font-size: 16px;
  }
  .modal h3 :global(svg) {
    color: var(--accent);
  }
  .mfoot {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 18px;
  }
  .sc {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 9px 16px;
    align-items: center;
  }
  .log {
    max-height: 50vh;
    overflow: auto;
    font-family: var(--mono);
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    user-select: text;
  }
  .log .err {
    color: var(--err);
  }
  .about {
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .alogo {
    width: 72px;
    height: 72px;
  }
  .about h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 24px;
  }
  .about h2 span {
    font-size: 14px;
    color: var(--text-3);
    font-weight: 500;
  }
  .credit {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 18px;
    border-radius: 12px;
    background: var(--accent-soft);
    border: 1px solid rgba(255, 170, 60, 0.35);
  }
  .credit span {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-2);
  }
  .about p {
    margin: 4px 0;
    color: var(--text-2);
    line-height: 1.5;
  }
  .arow {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .exit p {
    color: var(--text-2);
    line-height: 1.5;
    margin: 0 0 6px;
  }
  .ehead {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .ehead h3 {
    margin: 0;
  }
  .eicon {
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: rgba(255, 193, 77, 0.14);
    color: var(--warn);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .warnp {
    color: var(--warn) !important;
  }
  :global(.btn.danger) {
    color: #ffb1a8;
    border-color: rgba(255, 107, 107, 0.4);
    background: rgba(255, 107, 107, 0.08);
  }
  :global(.btn.danger:hover:not(:disabled)) {
    background: rgba(255, 107, 107, 0.18);
    border-color: rgba(255, 107, 107, 0.7);
  }
</style>
