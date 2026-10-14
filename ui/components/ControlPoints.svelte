<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { app, align, busyBlocking, type ManualPoint } from "../lib/store.svelte";

  interface Prev {
    data: string;
    w: number;
    h: number;
    fullW: number;
    fullH: number;
  }

  const ok = $derived(app.images.filter((x) => x.enabled && x.status === "ok"));
  let aId = $state<number>(app.cpPair?.[0] ?? ok[0]?.id ?? 0);
  let bId = $state<number>(app.cpPair?.[1] ?? ok[1]?.id ?? 0);
  const cache = new Map<number, Prev>();
  let prevA = $state<Prev | null>(null);
  let prevB = $state<Prev | null>(null);
  let pending = $state<{ x: number; y: number } | null>(null); // full-res point on A awaiting B

  async function load(id: number): Promise<Prev | null> {
    if (cache.has(id)) return cache.get(id)!;
    try {
      const p = await invoke<Prev>("work_preview", { id });
      cache.set(id, p);
      return p;
    } catch {
      return null;
    }
  }
  $effect(() => {
    const id = aId;
    load(id).then((p) => id === aId && (prevA = p));
  });
  $effect(() => {
    const id = bId;
    load(id).then((p) => id === bId && (prevB = p));
  });

  const pairPoints = $derived(
    app.manualPoints
      .map((m, idx) => ({ m, idx }))
      .filter(({ m }) => (m.a === aId && m.b === bId) || (m.a === bId && m.b === aId))
      .map(({ m, idx }) => (m.a === aId ? { idx, a: [m.ax, m.ay], b: [m.bx, m.by] } : { idx, a: [m.bx, m.by], b: [m.ax, m.ay] })),
  );

  // ---- per-pane zoom / pan
  type View = { z: number; x: number; y: number };
  let vA = $state<View>({ z: 1, x: 0, y: 0 });
  let vB = $state<View>({ z: 1, x: 0, y: 0 });
  let paneW = $state(500);
  let paneH = $state(400);

  function fitScale(p: Prev | null) {
    if (!p) return 1;
    return Math.min(paneW / p.fullW, paneH / p.fullH);
  }
  function toScreen(p: Prev, v: View, x: number, y: number) {
    const s = fitScale(p) * v.z;
    return [paneW / 2 + v.x + (x - p.fullW / 2) * s, paneH / 2 + v.y + (y - p.fullH / 2) * s];
  }
  function toImage(p: Prev, v: View, sx: number, sy: number) {
    const s = fitScale(p) * v.z;
    return [(sx - paneW / 2 - v.x) / s + p.fullW / 2, (sy - paneH / 2 - v.y) / s + p.fullH / 2];
  }

  let drag: { v: View; x: number; y: number; ox: number; oy: number; moved: boolean } | null = null;

  function down(e: PointerEvent, v: View) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { v, x: e.clientX, y: e.clientY, ox: v.x, oy: v.y, moved: e.button !== 0 };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.hypot(dx, dy) > 4) drag.moved = true;
    if (drag.moved) {
      drag.v.x = drag.ox + dx;
      drag.v.y = drag.oy + dy;
    }
  }
  function up(e: PointerEvent, side: "a" | "b") {
    const d = drag;
    drag = null;
    if (!d || d.moved || e.button !== 0) return;
    const p = side === "a" ? prevA : prevB;
    if (!p) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const [x, y] = toImage(p, side === "a" ? vA : vB, e.clientX - r.left, e.clientY - r.top);
    if (x < 0 || y < 0 || x > p.fullW || y > p.fullH) return;
    if (side === "a") pending = { x, y };
    else if (pending) {
      app.manualPoints.push({ a: aId, b: bId, ax: pending.x, ay: pending.y, bx: x, by: y } as ManualPoint);
      pending = null;
    }
  }
  function wheel(e: WheelEvent, v: View) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const mx = e.clientX - r.left - paneW / 2, my = e.clientY - r.top - paneH / 2;
    const f = Math.exp(-e.deltaY * 0.0018);
    const nz = Math.min(30, Math.max(1, v.z * f));
    const k = nz / v.z;
    v.x = mx - (mx - v.x) * k;
    v.y = my - (my - v.y) * k;
    v.z = nz;
  }
  function remove(idx: number) {
    app.manualPoints.splice(idx, 1);
  }
  function swapPair() {
    [aId, bId] = [bId, aId];
    pending = null;
  }
  async function apply() {
    app.dialogs.cp = false;
    await align();
  }
  const nameOf = (id: number) => app.images.find((x) => x.id === id)?.name ?? "?";
</script>

<div class="cp-bg">
  <div class="cp">
    <div class="head">
      <h3><Icon name="target" size={18} />{t("cpTitle")}</h3>
      <p class="muted">{t("cpHelp")}</p>
      <button class="btn ghost icon" onclick={() => (app.dialogs.cp = false)}><Icon name="x" size={18} /></button>
    </div>
    <div class="panes">
      {#each [{ side: "a", id: aId, prev: prevA, v: vA }, { side: "b", id: bId, prev: prevB, v: vB }] as pane (pane.side)}
        <div class="pane">
          <div class="pbar">
            <span class="lab">{pane.side === "a" ? "A" : "B"}</span>
            <select
              value={pane.id}
              onchange={(e) => {
                const v = +(e.target as HTMLSelectElement).value;
                if (pane.side === "a") aId = v;
                else bId = v;
                pending = null;
              }}
            >
              {#each ok as im}<option value={im.id}>{im.name}</option>{/each}
            </select>
            {#if pane.side === "a"}<button class="btn sm icon" data-tip={t("cpSwap")} onclick={swapPair}><Icon name="flipH" size={14} /></button>{/if}
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="view"
            bind:clientHeight={paneH}
            bind:clientWidth={paneW}
            onpointerdown={(e) => down(e, pane.v)}
            onpointermove={move}
            onpointerup={(e) => up(e, pane.side as "a" | "b")}
            onwheel={(e) => wheel(e, pane.v)}
            oncontextmenu={(e) => e.preventDefault()}
          >
            {#if pane.prev}
              {@const p = pane.prev}
              {@const s = fitScale(p) * pane.v.z}
              {@const o = toScreen(p, pane.v, 0, 0)}
              <img src={p.data} alt="" draggable="false" style="left:{o[0]}px;top:{o[1]}px;width:{p.fullW * s}px;height:{p.fullH * s}px" />
              <svg class="pts">
                {#each pairPoints as pp, n}
                  {@const c = toScreen(p, pane.v, pane.side === "a" ? pp.a[0] : pp.b[0], pane.side === "a" ? pp.a[1] : pp.b[1])}
                  <g transform="translate({c[0]},{c[1]})"><circle r="9" class="pc" /><line x1="-14" x2="14" class="ph" /><line y1="-14" y2="14" class="ph" /><text dy="-13" text-anchor="middle" class="pt">{n + 1}</text></g>
                {/each}
                {#if pending && pane.side === "a"}
                  {@const c = toScreen(p, pane.v, pending.x, pending.y)}
                  <g transform="translate({c[0]},{c[1]})"><circle r="11" class="pend" /><line x1="-16" x2="16" class="ph" /><line y1="-16" y2="16" class="ph" /></g>
                {/if}
              </svg>
            {:else}
              <span class="spin"></span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
    <div class="foot">
      <div class="status">
        {#if pending}
          <span class="dot"></span>{t("cpPickB")}
        {:else}
          {t("cpPickA")}
        {/if}
        <span class="muted"> · {nameOf(aId)} ↔ {nameOf(bId)}: {pairPoints.length} {t("cpPoints")}</span>
      </div>
      <div class="list">
        {#each pairPoints as pp, n}
          <button class="chip" data-tip={t("cpRemove")} onclick={() => remove(pp.idx)}>{n + 1} <Icon name="x" size={11} /></button>
        {/each}
      </div>
      <span class="grow"></span>
      <span class="muted small">{t("cpTotal")}: {app.manualPoints.length}</span>
      <button class="btn" onclick={() => (app.manualPoints = [])} disabled={!app.manualPoints.length}>{t("clear")}</button>
      <button class="btn primary" disabled={busyBlocking() || app.manualPoints.length < 2} onclick={apply}><Icon name="align" size={16} />{t("cpApply")}</button>
    </div>
  </div>
</div>

<style>
  .cp-bg {
    position: fixed;
    inset: 0;
    z-index: 120;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    padding: 24px;
  }
  .cp {
    flex: 1;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--line-2);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--line);
  }
  .head h3 {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 15px;
    white-space: nowrap;
  }
  .head p {
    flex: 1;
    margin: 0;
    font-size: 12px;
  }
  .panes {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1px;
    background: var(--line);
    min-height: 0;
  }
  .pane {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    min-height: 0;
  }
  .pbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
  }
  .pbar select {
    flex: 1;
  }
  .lab {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--sel-bg);
    color: var(--sel-fg);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .view {
    position: relative;
    flex: 1;
    overflow: hidden;
    background: var(--raised);
    cursor: crosshair;
    touch-action: none;
  }
  .view img {
    position: absolute;
    image-rendering: auto;
    pointer-events: none;
  }
  .pts {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .pc {
    fill: rgba(255, 255, 255, 0.15);
    stroke: #fff;
    stroke-width: 2;
  }
  .pend {
    fill: rgba(255, 59, 71, 0.2);
    stroke: #ff3b47;
    stroke-width: 2;
  }
  .ph {
    stroke: #000;
    stroke-width: 1;
  }
  .pt {
    fill: #fff;
    font-size: 12px;
    font-weight: 700;
    paint-order: stroke;
    stroke: #000;
    stroke-width: 3px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-top: 1px solid var(--line);
    flex-wrap: wrap;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ff3b47;
  }
  .list {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 22px;
    padding: 0 8px;
    border-radius: 999px;
    border: 1px solid var(--line-2);
    background: transparent;
    color: var(--text);
    font-size: 11px;
    cursor: pointer;
  }
  .grow {
    flex: 1;
  }
  .spin {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 22px;
    height: 22px;
    margin: -11px;
    border-radius: 50%;
    border: 2px solid var(--line-2);
    border-top-color: var(--text);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
