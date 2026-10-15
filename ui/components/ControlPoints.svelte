<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import { t, countPoints } from "../lib/i18n.svelte";
  import { app, align, busyBlocking, readyIds, imageName, IS_MOBILE, type ManualPoint, type PairReport } from "../lib/store.svelte";

  interface Prev {
    data: string;
    w: number;
    h: number;
    fullW: number;
    fullH: number;
  }
  interface MatchPt {
    ax: number;
    ay: number;
    bx: number;
    by: number;
    inlier: boolean;
    error: number | null;
  }
  interface PairMatches {
    tested: boolean;
    accepted: boolean;
    solved: boolean;
    auto: MatchPt[];
    manual: (number | null)[];
  }

  /** error (px) above which a point is drawn as a wrong match */
  const BAD_PX = 4;
  /** error (px) above which a user's control point is flagged */
  const BAD_MANUAL_PX = 10;

  const ok = $derived(app.images.filter((x) => x.enabled && x.status === "ok"));
  const start = readyIds();
  let aId = $state<number>(app.cpPair?.[0] ?? start[0] ?? 0);
  let bId = $state<number>(app.cpPair?.[1] ?? start[1] ?? 0);
  const cache = new Map<number, Prev>();
  let prevA = $state<Prev | null>(null);
  let prevB = $state<Prev | null>(null);
  let pending = $state<{ x: number; y: number } | null>(null); // full-res point on A awaiting B
  let predicted = $state<[number, number] | null>(null); // where the pending point should land on B
  let showAuto = $state(true);
  let loupeOn = $state(true);
  let onlyProblems = $state(true);
  let hoverAuto = $state<number | null>(null);

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
      .map(({ m, idx }) => (m.a === aId ? { idx, m, a: [m.ax, m.ay], b: [m.bx, m.by] } : { idx, m, a: [m.bx, m.by], b: [m.ax, m.ay] })),
  );

  // ---- diagnostics of the selected pair
  const hasDiag = $derived(!!app.report && (!!app.align || app.alignFailed));
  let matches = $state<PairMatches | null>(null);
  $effect(() => {
    const a = aId, b = bId;
    const manual = pairPoints.map((p) => $state.snapshot(p.m));
    if (!hasDiag || a === b) {
      matches = null;
      return;
    }
    invoke<PairMatches>("pair_matches", { a, b, manual })
      .then((r) => a === aId && b === bId && (matches = r))
      .catch(() => (matches = null));
  });
  const isBad = (m: MatchPt) => !m.inlier || (m.error != null && m.error > BAD_PX);
  const autoCounts = $derived.by(() => {
    const list = matches?.auto ?? [];
    const bad = list.filter(isBad).length;
    return { good: list.length - bad, bad };
  });
  const manualErr = (n: number) => matches?.manual[n] ?? null;

  const pairReport = (a: number, b: number) => app.report?.pairs.find((p) => (p.a === a && p.b === b) || (p.a === b && p.b === a)) ?? null;
  const cur = $derived(pairReport(aId, bId));

  // pairs worth fixing: inconsistent, rejected, or touching an unaligned image
  const listed = $derived.by(() => {
    const r = app.report;
    if (!r) return [] as (PairReport & { unaligned: boolean })[];
    const valid = new Set(ok.map((x) => x.id));
    const un = new Set(r.unaligned);
    const all = r.pairs
      .filter((p) => valid.has(p.a) && valid.has(p.b))
      .map((p) => ({ ...p, unaligned: un.has(p.a) || un.has(p.b) }));
    if (!onlyProblems) return all;
    return all.filter((p) => p.status === "highError" || p.status === "rejected");
  });
  // unaligned images that were never matched against anything
  const lonelyImages = $derived.by(() => {
    const r = app.report;
    if (!r) return [] as number[];
    return r.unaligned.filter((id) => ok.some((x) => x.id === id) && !r.pairs.some((p) => (p.a === id || p.b === id) && p.status !== "weak"));
  });
  function pickPair(a: number, b: number) {
    aId = a;
    bId = b;
    pending = null;
    vA = { z: 1, x: 0, y: 0 };
    vB = { z: 1, x: 0, y: 0 };
  }
  function pickLonely(id: number) {
    const ids = ok.map((x) => x.id);
    const k = ids.indexOf(id);
    const nb = ids[k > 0 ? k - 1 : k + 1];
    if (nb != null) pickPair(nb, id);
  }

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

  // ---- pointer handling: mouse click places a point, drag pans; touch: drag pans, press & hold
  // switches to precise placement with the loupe above the finger, release places the point.
  type Side = "a" | "b";
  let drag: { v: View; x: number; y: number; ox: number; oy: number; moved: boolean; touch: boolean; precise: boolean; timer: any } | null = null;
  let hover = $state<{ side: Side; x: number; y: number; touch: boolean } | null>(null);
  const TOUCH_LIFT = 90; // px the touch target sits above the finger

  function local(e: PointerEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }
  function down(e: PointerEvent, side: Side) {
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {}
    const v = side === "a" ? vA : vB;
    const touch = e.pointerType === "touch";
    const d = { v, x: e.clientX, y: e.clientY, ox: v.x, oy: v.y, moved: e.button !== 0, touch, precise: false, timer: null as any };
    if (touch) {
      const [lx, ly] = local(e);
      d.timer = setTimeout(() => {
        if (drag === d && !d.moved) {
          d.precise = true;
          hover = { side, x: lx, y: ly - TOUCH_LIFT, touch: true };
        }
      }, 260);
    }
    drag = d;
  }
  function move(e: PointerEvent, side: Side) {
    const [lx, ly] = local(e);
    if (drag?.precise) {
      hover = { side, x: lx, y: ly - TOUCH_LIFT, touch: true };
      return;
    }
    if (!drag) {
      if (e.pointerType !== "touch") hover = { side, x: lx, y: ly, touch: false };
      return;
    }
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.hypot(dx, dy) > 4) drag.moved = true;
    if (drag.moved) {
      drag.v.x = drag.ox + dx;
      drag.v.y = drag.oy + dy;
      hover = null;
    } else if (!drag.touch) hover = { side, x: lx, y: ly, touch: false };
  }
  function up(e: PointerEvent, side: Side) {
    const d = drag;
    drag = null;
    if (!d) return;
    clearTimeout(d.timer);
    let pos: number[];
    if (d.precise && hover) {
      pos = [hover.x, hover.y];
      hover = null;
    } else {
      if (d.moved || e.button !== 0) return;
      pos = local(e);
    }
    place(side, pos[0], pos[1]);
  }
  function place(side: Side, sx: number, sy: number) {
    const p = side === "a" ? prevA : prevB;
    if (!p) return;
    const [x, y] = toImage(p, side === "a" ? vA : vB, sx, sy);
    if (x < 0 || y < 0 || x > p.fullW || y > p.fullH) return;
    if (side === "a") {
      pending = { x, y };
      predict(x, y);
    } else if (pending) {
      app.manualPoints.push({ a: aId, b: bId, ax: pending.x, ay: pending.y, bx: x, by: y } as ManualPoint);
      pending = null;
      predicted = null;
    }
  }
  async function predict(x: number, y: number) {
    predicted = null;
    if (!hasDiag) return;
    const a = aId, b = bId;
    const r = await invoke<[number, number] | null>("predict_point", { a, b, x, y }).catch(() => null);
    if (!r || a !== aId || b !== bId || !pending) return;
    if (prevB && (r[0] < 0 || r[1] < 0 || r[0] > prevB.fullW || r[1] > prevB.fullH)) return;
    predicted = r;
    // bring the predicted spot into view on B and zoom in a little
    if (prevB) {
      const [sx, sy] = toScreen(prevB, vB, r[0], r[1]);
      if (vB.z < 2.5 || sx < 40 || sy < 40 || sx > paneW - 40 || sy > paneH - 40) {
        const z = Math.max(vB.z, 3);
        const s = fitScale(prevB) * z;
        vB = { z, x: -(r[0] - prevB.fullW / 2) * s, y: -(r[1] - prevB.fullH / 2) * s };
      }
    }
  }
  function leave() {
    if (!drag?.precise) hover = null;
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
  function zoomBy(v: View, f: number) {
    v.z = Math.min(30, Math.max(1, v.z * f));
    if (v.z === 1) v.x = v.y = 0;
  }

  // ---- loupe
  const LOUPE = IS_MOBILE ? 130 : 170;
  function loupeFor(side: Side) {
    if (!loupeOn || !hover || hover.side !== side) return null;
    const p = side === "a" ? prevA : prevB;
    if (!p) return null;
    const v = side === "a" ? vA : vB;
    const [ix, iy] = toImage(p, v, hover.x, hover.y);
    if (ix < 0 || iy < 0 || ix > p.fullW || iy > p.fullH) return null;
    const workPerFull = p.w / p.fullW;
    const screenPerWork = (fitScale(p) * v.z) / workPerFull;
    const k = Math.min(16, Math.max(5, screenPerWork * 4)); // loupe px per work px
    // place the loupe away from the cursor/finger, flipping near the edges
    let lx = hover.touch ? hover.x - LOUPE / 2 : hover.x + 24;
    let ly = hover.touch ? hover.y - LOUPE - 24 : hover.y - LOUPE - 24;
    if (lx + LOUPE > paneW - 4) lx = hover.x - LOUPE - 24;
    if (lx < 4) lx = 4;
    if (ly < 4) {
      if (hover.touch) {
        // no room above: put it beside the finger so it does not cover it
        lx = hover.x + LOUPE + 30 < paneW ? hover.x + 30 : Math.max(4, hover.x - LOUPE - 30);
        ly = Math.max(4, Math.min(paneH - LOUPE - 4, hover.y - LOUPE / 2));
      } else ly = hover.y + 24;
    }
    return {
      lx,
      ly,
      w: p.w * k,
      h: p.h * k,
      ox: LOUPE / 2 - ix * workPerFull * k,
      oy: LOUPE / 2 - iy * workPerFull * k,
      src: p.data,
      fx: hover.x,
      fy: hover.y,
      // full-res px per loupe px – used to place markers inside the loupe
      scale: workPerFull * k,
      ix,
      iy,
    };
  }

  function remove(idx: number) {
    app.manualPoints.splice(idx, 1);
  }
  function swapPair() {
    [aId, bId] = [bId, aId];
    [vA, vB] = [vB, vA];
    pending = null;
    predicted = null;
  }
  async function apply() {
    app.dialogs.cp = false;
    await align();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      if (pending) {
        pending = null;
        predicted = null;
      } else app.dialogs.cp = false;
    }
  }
  const statusLabel = (s: string) => t(("cpStatus_" + s) as any);
  const pairLine = (p: PairReport) =>
    `${p.matches} ${t("qMatchesOf")} ${p.inliers}${p.error != null ? ` · ${t("cpErr")} ${p.error.toFixed(1)} px` : ""}`;
</script>

<svelte:window onkeydown={onKey} />

<div class="cp-bg">
  <div class="cp" class:mobile={IS_MOBILE}>
    <div class="head">
      <h3><Icon name="target" size={18} />{t("cpTitle")}</h3>
      <p class="muted">{t("cpHelp")}</p>
      <button class="btn ghost icon" onclick={() => (app.dialogs.cp = false)}><Icon name="x" size={18} /></button>
    </div>
    <div class="body">
      {#if app.report}
        <aside class="pairs">
          <div class="ptabs seg full">
            <button class:on={onlyProblems} onclick={() => (onlyProblems = true)}>{t("cpPairs")}</button>
            <button class:on={!onlyProblems} onclick={() => (onlyProblems = false)}>{t("cpAllPairs")}</button>
          </div>
          <div class="plist">
            {#each lonelyImages as id}
              <button class="prow" class:sel={bId === id || aId === id} onclick={() => pickLonely(id)}>
                <span class="st none"></span>
                <span class="pn"><b>{imageName(id)}</b><small>{t("qNotConnected")} · {t("cpStatus_none")}</small></span>
              </button>
            {/each}
            {#each listed as p}
              <button class="prow" class:sel={(p.a === aId && p.b === bId) || (p.a === bId && p.b === aId)} onclick={() => pickPair(p.a, p.b)}>
                <span class="st {p.status}"></span>
                <span class="pn">
                  <b>{imageName(p.a)} ↔ {imageName(p.b)}</b>
                  <small>{statusLabel(p.status)}{p.unaligned ? ` · ${t("qNotConnected")}` : ""} · {pairLine(p)}</small>
                </span>
              </button>
            {:else}
              {#if !lonelyImages.length}<div class="pempty muted">—</div>{/if}
            {/each}
          </div>
        </aside>
      {/if}
      <div class="panes">
        {#each [{ side: "a", id: aId, prev: prevA, v: vA }, { side: "b", id: bId, prev: prevB, v: vB }] as pane (pane.side)}
          {@const side = pane.side as Side}
          {@const lp = loupeFor(side)}
          <div class="pane">
            <div class="pbar">
              <span class="lab">{side === "a" ? "A" : "B"}</span>
              <select
                value={pane.id}
                onchange={(e) => {
                  const v = +(e.target as HTMLSelectElement).value;
                  if (side === "a") aId = v;
                  else bId = v;
                  pending = null;
                  predicted = null;
                }}
              >
                {#each ok as im}<option value={im.id}>{im.name}</option>{/each}
              </select>
              <button class="btn sm icon" data-tip={t("zoomOut")} onclick={() => zoomBy(pane.v, 1 / 1.6)}><Icon name="zoomOut" size={14} /></button>
              <button class="btn sm icon" data-tip={t("zoomIn")} onclick={() => zoomBy(pane.v, 1.6)}><Icon name="zoomIn" size={14} /></button>
              {#if side === "a"}<button class="btn sm icon" data-tip={t("cpSwap")} onclick={swapPair}><Icon name="flipH" size={14} /></button>{/if}
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="view"
              class:hideCursor={loupeOn && hover?.side === side && !hover.touch}
              bind:clientHeight={paneH}
              bind:clientWidth={paneW}
              onpointerdown={(e) => down(e, side)}
              onpointermove={(e) => move(e, side)}
              onpointerup={(e) => up(e, side)}
              onpointercancel={() => ((drag = null), (hover = null))}
              onpointerleave={leave}
              onwheel={(e) => wheel(e, pane.v)}
              oncontextmenu={(e) => e.preventDefault()}
            >
              {#if pane.prev}
                {@const p = pane.prev}
                {@const s = fitScale(p) * pane.v.z}
                {@const o = toScreen(p, pane.v, 0, 0)}
                <img src={p.data} alt="" draggable="false" style="left:{o[0]}px;top:{o[1]}px;width:{p.fullW * s}px;height:{p.fullH * s}px" />
                <svg class="pts">
                  {#if showAuto && matches}
                    {#each matches.auto as m, k}
                      {@const c = toScreen(p, pane.v, side === "a" ? m.ax : m.bx, side === "a" ? m.ay : m.by)}
                      <circle cx={c[0]} cy={c[1]} r={hoverAuto === k ? 7 : IS_MOBILE ? 2.2 : 3.2} class="ap" class:bad={isBad(m)} class:hl={hoverAuto === k} />
                    {/each}
                  {/if}
                  {#each pairPoints as pp, n}
                    {@const c = toScreen(p, pane.v, side === "a" ? pp.a[0] : pp.b[0], side === "a" ? pp.a[1] : pp.b[1])}
                    {@const err = manualErr(n)}
                    <g transform="translate({c[0]},{c[1]})" class:badm={err != null && err > BAD_MANUAL_PX}><circle r="9" class="pc" /><line x1="-14" x2="14" class="ph" /><line y1="-14" y2="14" class="ph" /><text dy="-13" text-anchor="middle" class="pt">{n + 1}</text></g>
                  {/each}
                  {#if pending && side === "a"}
                    {@const c = toScreen(p, pane.v, pending.x, pending.y)}
                    <g transform="translate({c[0]},{c[1]})"><circle r="11" class="pend" /><line x1="-16" x2="16" class="ph" /><line y1="-16" y2="16" class="ph" /></g>
                  {/if}
                  {#if predicted && pending && side === "b"}
                    {@const c = toScreen(p, pane.v, predicted[0], predicted[1])}
                    <g transform="translate({c[0]},{c[1]})"><circle r="16" class="pred" /><text dy="-21" text-anchor="middle" class="pt small">{t("cpPredicted")}</text></g>
                  {/if}
                </svg>
                {#if showAuto && matches && !IS_MOBILE}
                  <!-- hover targets for automatic points: highlight the partner on the other image -->
                  <svg class="pts hit">
                    {#each matches.auto as m, k}
                      {@const c = toScreen(p, pane.v, side === "a" ? m.ax : m.bx, side === "a" ? m.ay : m.by)}
                      <circle cx={c[0]} cy={c[1]} r="6" role="presentation" onpointerenter={() => (hoverAuto = k)} onpointerleave={() => (hoverAuto = null)} />
                    {/each}
                  </svg>
                {/if}
                {#if lp}
                  <div class="loupe" style="left:{lp.lx}px;top:{lp.ly}px;width:{LOUPE}px;height:{LOUPE}px">
                    <img src={lp.src} alt="" draggable="false" style="width:{lp.w}px;height:{lp.h}px;transform:translate({lp.ox}px,{lp.oy}px)" />
                    <svg class="lmarks" viewBox="0 0 {LOUPE} {LOUPE}">
                      {#each pairPoints as pp}
                        {@const q = side === "a" ? pp.a : pp.b}
                        <circle cx={LOUPE / 2 + (q[0] - lp.ix) * lp.scale} cy={LOUPE / 2 + (q[1] - lp.iy) * lp.scale} r="7" class="pc" />
                      {/each}
                      {#if predicted && pending && side === "b"}
                        <circle cx={LOUPE / 2 + (predicted[0] - lp.ix) * lp.scale} cy={LOUPE / 2 + (predicted[1] - lp.iy) * lp.scale} r="12" class="pred" />
                      {/if}
                      <line x1={LOUPE / 2} x2={LOUPE / 2} y1="0" y2={LOUPE / 2 - 5} class="lc" />
                      <line x1={LOUPE / 2} x2={LOUPE / 2} y1={LOUPE / 2 + 5} y2={LOUPE} class="lc" />
                      <line y1={LOUPE / 2} y2={LOUPE / 2} x1="0" x2={LOUPE / 2 - 5} class="lc" />
                      <line y1={LOUPE / 2} y2={LOUPE / 2} x1={LOUPE / 2 + 5} x2={LOUPE} class="lc" />
                      <circle cx={LOUPE / 2} cy={LOUPE / 2} r="1.6" class="ldot" />
                    </svg>
                  </div>
                  {#if hover?.touch}
                    <svg class="pts"><g transform="translate({lp.fx},{lp.fy})"><circle r="7" class="pend" /><line x1="-11" x2="11" class="ph" /><line y1="-11" y2="11" class="ph" /></g></svg>
                  {/if}
                {/if}
              {:else}
                <span class="spin"></span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
    <div class="foot">
      <div class="status">
        {#if pending}
          <span class="dot"></span>{t("cpPickB")}
        {:else}
          {t("cpPickA")}
        {/if}
        <span class="muted">
          · {imageName(aId)} ↔ {imageName(bId)}: {countPoints(pairPoints.length)}{#if pairPoints.length === 1} ({t("cpNeed2")}){/if}
          {#if cur} · {statusLabel(cur.status)} ({pairLine(cur)}){/if}
        </span>
      </div>
      <div class="list">
        {#each pairPoints as pp, n}
          {@const err = manualErr(n)}
          <button
            class="chip"
            class:badc={err != null && err > BAD_MANUAL_PX}
            data-tip={err != null && err > BAD_MANUAL_PX ? `${t("cpBadPoint")} (${err.toFixed(0)} px) · ${t("cpRemove")}` : t("cpRemove")}
            onclick={() => remove(pp.idx)}>{n + 1}{#if err != null}<small>{err.toFixed(0)} px</small>{/if} <Icon name="x" size={11} /></button
          >
        {/each}
      </div>
      <span class="grow"></span>
      {#if matches?.auto.length}
        <label class="tog" data-tip={t("cpShowAutoHint")}>
          <input type="checkbox" bind:checked={showAuto} />{t("cpShowAuto")}
          <span class="lg"><i class="g"></i>{autoCounts.good} {t("cpLegendGood")}</span>
          <span class="lg"><i class="r"></i>{autoCounts.bad} {t("cpLegendBad")}</span>
        </label>
      {/if}
      <label class="tog" data-tip={t("cpLoupeHint")}><input type="checkbox" bind:checked={loupeOn} />{t("cpLoupe")}</label>
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
    min-width: 0;
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
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .pairs {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    box-shadow: 1px 0 0 var(--line);
    z-index: 1;
    min-height: 0;
  }
  .ptabs {
    margin: 8px;
  }
  .plist {
    flex: 1;
    overflow: auto;
    padding: 0 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .pempty {
    padding: 10px;
    text-align: center;
  }
  .prow {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 7px 8px;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .prow:hover {
    background: var(--raised-2);
  }
  .prow.sel {
    background: var(--raised);
    border-color: var(--line-2);
  }
  .pn {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .pn b {
    font-weight: 600;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pn small {
    font-size: 11px;
    color: var(--text-3);
  }
  .st {
    width: 9px;
    height: 9px;
    margin-top: 4px;
    border-radius: 50%;
    flex: none;
    background: var(--ok);
  }
  .st.highError,
  .st.rejected,
  .st.none {
    background: var(--err);
  }
  .st.weak {
    background: var(--text-3);
  }
  .panes {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1px;
    background: var(--line);
    min-height: 0;
    min-width: 0;
  }
  .pane {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    min-height: 0;
    min-width: 0;
  }
  .pbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
  }
  .pbar select {
    flex: 1;
    min-width: 0;
  }
  .lab {
    width: 24px;
    height: 24px;
    flex: none;
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
  .view.hideCursor {
    cursor: none;
  }
  .view > img {
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
  .pts.hit circle {
    fill: transparent;
    pointer-events: all;
    cursor: crosshair;
  }
  .ap {
    fill: #35d07f;
    stroke: rgba(0, 0, 0, 0.7);
    stroke-width: 1;
  }
  .ap.bad {
    fill: #ff3b47;
  }
  .ap.hl {
    fill: none;
    stroke: #fff;
    stroke-width: 2.5;
  }
  .pc {
    fill: rgba(255, 255, 255, 0.15);
    stroke: #fff;
    stroke-width: 2;
  }
  .badm .pc {
    stroke: #ff3b47;
    fill: rgba(255, 59, 71, 0.25);
  }
  .pend {
    fill: rgba(255, 59, 71, 0.2);
    stroke: #ff3b47;
    stroke-width: 2;
  }
  .pred {
    fill: none;
    stroke: #ffd24d;
    stroke-width: 2;
    stroke-dasharray: 5 4;
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
  .pt.small {
    font-size: 10.5px;
    font-weight: 600;
  }
  .loupe {
    position: absolute;
    border-radius: 50%;
    overflow: hidden;
    pointer-events: none;
    border: 2px solid #fff;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6), 0 10px 30px rgba(0, 0, 0, 0.55);
    background: var(--raised);
    z-index: 3;
  }
  .loupe img {
    position: absolute;
    left: 0;
    top: 0;
    max-width: none;
    transform-origin: 0 0;
  }
  .lmarks {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .lc {
    stroke: #ff3b47;
    stroke-width: 1.2;
  }
  .ldot {
    fill: #ff3b47;
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
    flex-wrap: wrap;
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
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border-radius: 999px;
    border: 1px solid var(--line-2);
    background: transparent;
    color: var(--text);
    font-size: 11px;
    cursor: pointer;
  }
  .chip small {
    color: var(--text-3);
    font-size: 10px;
  }
  .chip.badc {
    border-color: var(--err);
    color: var(--err);
  }
  .tog {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
  }
  .lg {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text-3);
  }
  .lg i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .lg .g {
    background: #35d07f;
  }
  .lg .r {
    background: #ff3b47;
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
  /* narrow screens: pair list on top, images stacked */
  @media (max-width: 820px) {
    .cp-bg {
      padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom);
    }
    .cp {
      border-radius: 0;
    }
    .head p {
      display: none;
    }
    .body {
      flex-direction: column;
    }
    .pairs {
      width: auto;
      max-height: 30%;
      box-shadow: 0 1px 0 var(--line);
    }
    .panes {
      grid-template-columns: 1fr;
      grid-template-rows: 1fr 1fr;
    }
    .foot {
      padding: 8px 10px;
      gap: 8px;
    }
  }
</style>
