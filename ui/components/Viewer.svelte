<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import NavPad from "./NavPad.svelte";
  import { SphereRenderer } from "../lib/gl";
  import { wraps, forward, inverse, lensRay, level, largestRect, mul, solveGrab, transpose, ypr, yawPitchTowards, type V3 } from "../lib/geometry";
  import { t } from "../lib/i18n.svelte";
  import { S, app, vw, fitView, viewChanged, commitHistory, undo, redo, canUndo, canRedo, persist, type Tool } from "../lib/store.svelte";

  let host: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let renderer: SphereRenderer | null = null;
  let glError = $state<string | null>(null);
  let W = $state(800);
  let H = $state(600);
  let dpr = $state(1);
  let hover = $state<{ x: number; y: number } | null>(null);
  let points = $state<V3[]>([]);
  let frozen = $state<[number, number, number, number] | null>(null);
  type Rc = { x: number; y: number; k: number; w: number; h: number };
  let rectFrozen: Rc | null = null;
  let uploadedVersion = -1;

  const r = $derived(S.settings.render);
  const L = $derived(app.layout);
  const bounds = $derived<[number, number, number, number] | null>(frozen ?? (L ? [L.u0, L.v0, L.u1, L.v1] : null));
  const proj = $derived(L?.projection ?? "Cylindrical");

  // content rect in CSS px
  const rect = $derived.by((): Rc | null => {
    if (!bounds) return null;
    if (rectFrozen) return rectFrozen;
    const bw = bounds[2] - bounds[0];
    const bh = bounds[3] - bounds[1];
    const pad = 28;
    const fit = Math.max(1e-6, Math.min((W - pad * 2) / bw, (H - pad * 2) / bh));
    const k = fit * vw.zoom;
    const x = W / 2 - (bw * k) / 2 + vw.panX;
    const y = H / 2 - (bh * k) / 2 + vw.panY;
    return { x, y, k, w: bw * k, h: bh * k };
  });

  const R = $derived(ypr(r.yaw, r.pitch, r.roll));

  function screenToWorld(sx: number, sy: number, rc = rect, b = bounds): V3 | null {
    if (!rc || !b) return null;
    let rx = (sx - rc.x) / rc.k;
    let ry = (sy - rc.y) / rc.k;
    const bw = b[2] - b[0], bh = b[3] - b[1];
    if (rx < 0 || ry < 0 || rx > bw || ry > bh) return null;
    if (r.flip_h) rx = bw - rx;
    if (r.flip_v) ry = bh - ry;
    const q = inverse(proj, b[0] + rx, b[1] + ry);
    return q ? mul(R, q) : null;
  }

  function planeDirAt(sx: number, sy: number, rc: { x: number; y: number; k: number }, b: [number, number, number, number]): V3 | null {
    let rx = (sx - rc.x) / rc.k;
    let ry = (sy - rc.y) / rc.k;
    const bw = b[2] - b[0], bh = b[3] - b[1];
    if (r.flip_h) rx = bw - rx;
    if (r.flip_v) ry = bh - ry;
    return inverse(proj, b[0] + rx, b[1] + ry);
  }

  function worldToScreen(d: V3): [number, number] | null {
    if (!rect || !bounds) return null;
    const q = mul(transpose(R), d);
    const uv = forward(proj, q);
    if (!uv) return null;
    const bw = bounds[2] - bounds[0], bh = bounds[3] - bounds[1];
    let rx = uv[0] - bounds[0];
    let ry = uv[1] - bounds[1];
    if (r.flip_h) rx = bw - rx;
    if (r.flip_v) ry = bh - ry;
    return [rect.x + rx * rect.k, rect.y + ry * rect.k];
  }

  // ------------------------------------------------------------------ drawing

  let raf = 0;
  function schedule() {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      draw();
    });
  }

  function draw() {
    if (!renderer) return;
    if (app.sphere && app.sphere.version !== uploadedVersion) {
      renderer.setSphere(app.sphere.meta, app.sphere.pixels);
      uploadedVersion = app.sphere.version;
    }
    if (!app.sphere) {
      renderer.clearSphere();
      uploadedVersion = -1;
    }
    if (!rect || !bounds) {
      renderer.draw({ originX: 0, originY: 0, k: 1, bounds: [0, 0, 1, 1], projection: proj, R, flipH: false, flipV: false, dpr });
      return;
    }
    renderer.draw({ originX: rect.x * dpr, originY: rect.y * dpr, k: rect.k * dpr, bounds, projection: proj, R, flipH: r.flip_h, flipV: r.flip_v, dpr });
  }

  $effect(() => {
    // dependencies
    void [rect, bounds, R, proj, r.flip_h, r.flip_v, app.sphere, W, H, dpr];
    schedule();
  });

  // auto-crop estimate from the GPU coverage mask (not during interaction)
  let cropTimer: any = null;
  $effect(() => {
    void [L, R, r.flip_h, r.flip_v, app.sphere];
    if (vw.interacting || frozen) return;
    clearTimeout(cropTimer);
    cropTimer = setTimeout(computeAutoCrop, 120);
  });

  function computeAutoCrop() {
    if (!renderer || !app.sphere || !L) {
      app.autoCrop = null;
      return;
    }
    const b: [number, number, number, number] = [L.u0, L.v0, L.u1, L.v1];
    const bw = b[2] - b[0], bh = b[3] - b[1];
    const cw = 900;
    const ch = Math.max(8, Math.round((cw * bh) / bw));
    const mask = renderer.coverage({ originX: 0, originY: 0, k: 1, bounds: b, projection: L.projection, R, flipH: r.flip_h, flipV: r.flip_v, dpr: 1 }, cw, Math.min(ch, 4000));
    schedule();
    if (!mask) return;
    const hh = Math.min(ch, 4000);
    const rc = largestRect(mask, cw, hh);
    if (!rc) {
      app.autoCrop = null;
      return;
    }
    const [x, y, w, h] = rc;
    app.autoCrop = [(x + 1) / cw, (y + 1) / hh, (cw - x - w + 1) / cw, (hh - y - h + 1) / hh];
  }

  // ------------------------------------------------------------------ overlays

  function polyline(pts: ([number, number] | null)[], maxJump: number): string {
    let d = "";
    let prev: [number, number] | null = null;
    for (const p of pts) {
      if (!p) {
        prev = null;
        continue;
      }
      if (prev && Math.hypot(p[0] - prev[0], p[1] - prev[1]) < maxJump) d += `L${p[0].toFixed(1)},${p[1].toFixed(1)}`;
      else d += `M${p[0].toFixed(1)},${p[1].toFixed(1)}`;
      prev = p;
    }
    return d;
  }

  const D2R = Math.PI / 180;
  const sph = (lon: number, lat: number): V3 => [Math.cos(lat * D2R) * Math.sin(lon * D2R), -Math.sin(lat * D2R), Math.cos(lat * D2R) * Math.cos(lon * D2R)];

  const grid = $derived.by(() => {
    if (!S.settings.ui.showGrid || !rect) return null;
    const jump = Math.max(rect.w, rect.h) * 0.3;
    const lats: string[] = [];
    for (const lat of [-60, -30, 30, 60]) {
      const pts = [];
      for (let k = 0; k <= 360; k += 2) pts.push(worldToScreen(sph(-180 + k, lat)));
      lats.push(polyline(pts, jump));
    }
    const lons: string[] = [];
    let meridian = "";
    for (let lon = -180; lon < 180; lon += 30) {
      const pts = [];
      for (let k = 0; k <= 178; k += 2) pts.push(worldToScreen(sph(lon, -89 + k)));
      const d = polyline(pts, jump);
      if (lon === 0) meridian = d;
      else lons.push(d);
    }
    const hp = [];
    for (let k = 0; k <= 720; k++) hp.push(worldToScreen(sph(-180 + k * 0.5, 0)));
    return { lats, lons, meridian, horizon: polyline(hp, jump) };
  });

  const outlines = $derived.by(() => {
    const al = app.align;
    if (!al || !rect) return [];
    const sel = app.selectedId;
    if (!S.settings.ui.showOutlines && sel === null) return [];
    const jump = Math.max(rect.w, rect.h) * 0.3;
    const out: { d: string; label: [number, number] | null; n: number; sel: boolean }[] = [];
    al.cameras.forEach((c, i) => {
      if (!c.aligned) return;
      const isSel = c.id === sel;
      if (!S.settings.ui.showOutlines && !isSel) return;
      const w = c.w - 1, h = c.h - 1, cx = w / 2, cy = h / 2;
      const ray = (x: number, y: number) => mul(c.rot, lensRay(al.lens, x - cx, y - cy, c.f));
      const pts: ([number, number] | null)[] = [];
      const n = 32;
      const edges: [number, number, number, number][] = [
        [0, 0, w, 0],
        [w, 0, w, h],
        [w, h, 0, h],
        [0, h, 0, 0],
      ];
      for (const [x0, y0, x1, y1] of edges) for (let k = 0; k <= n; k++) pts.push(worldToScreen(ray(x0 + ((x1 - x0) * k) / n, y0 + ((y1 - y0) * k) / n)));
      out.push({ d: polyline(pts, jump), label: worldToScreen(ray(cx, cy)), n: i + 1, sel: isSel });
    });
    return out;
  });

  const cropMargins = $derived.by((): [number, number, number, number] | null => {
    if (r.crop === "None") return null;
    if (r.crop === "Manual") return r.manual_crop;
    return app.autoCrop;
  });
  const cropRect = $derived.by(() => {
    if (!rect || !cropMargins) return null;
    const [l, tp, rt, b] = cropMargins;
    return { x: rect.x + l * rect.w, y: rect.y + tp * rect.h, w: rect.w * (1 - l - rt), h: rect.h * (1 - tp - b) };
  });

  // ------------------------------------------------------------------ interaction

  type Drag =
    | { kind: "grab"; d0: V3; b: [number, number, number, number]; rc: { x: number; y: number; k: number } }
    | { kind: "roll"; a0: number; roll0: number; cx: number; cy: number }
    | { kind: "pan"; x: number; y: number; px: number; py: number }
    | { kind: "cropNew"; x: number; y: number }
    | { kind: "cropEdge"; l: boolean; t: boolean; r: boolean; b: boolean };
  let drag: Drag | null = null;
  let downAt: { x: number; y: number } | null = null;
  let moved = false;

  function local(e: PointerEvent | WheelEvent | MouseEvent) {
    const b = host.getBoundingClientRect();
    return { x: e.clientX - b.left, y: e.clientY - b.top };
  }

  function nearEdges(p: { x: number; y: number }) {
    if (!cropRect) return null;
    const tol = 9;
    const c = cropRect;
    const inY = p.y > c.y - tol && p.y < c.y + c.h + tol;
    const inX = p.x > c.x - tol && p.x < c.x + c.w + tol;
    const e = {
      l: inY && Math.abs(p.x - c.x) < tol,
      r: inY && Math.abs(p.x - (c.x + c.w)) < tol,
      t: inX && Math.abs(p.y - c.y) < tol,
      b: inX && Math.abs(p.y - (c.y + c.h)) < tol,
    };
    return e.l || e.r || e.t || e.b ? e : null;
  }

  function startInteraction() {
    if (!bounds || !rect) return;
    frozen = [...bounds] as [number, number, number, number];
    rectFrozen = { ...rect };
    vw.interacting = true;
  }
  function endInteraction() {
    frozen = null;
    rectFrozen = null;
    vw.interacting = false;
    viewChanged();
  }

  function onPointerDown(e: PointerEvent) {
    if (!rect || !bounds) return;
    host.setPointerCapture(e.pointerId);
    const p = local(e);
    downAt = p;
    moved = false;
    const tool: Tool = vw.tool;
    if (e.button === 1 || tool === "pan" || (e.button === 0 && e.altKey)) {
      drag = { kind: "pan", x: p.x, y: p.y, px: vw.panX, py: vw.panY };
      return;
    }
    if (tool === "rotate") {
      if (e.button === 2 || e.shiftKey) {
        startInteraction();
        const cx = rect.x + rect.w / 2, cy = rect.y + rect.h / 2;
        drag = { kind: "roll", a0: Math.atan2(p.y - cy, p.x - cx), roll0: r.roll, cx, cy };
      } else if (e.button === 0) {
        const d0 = screenToWorld(p.x, p.y);
        if (d0) {
          startInteraction();
          drag = { kind: "grab", d0, b: frozen!, rc: { x: rect.x, y: rect.y, k: rect.k } };
        }
      }
      return;
    }
    if (tool === "crop" && e.button === 0) {
      const ed = nearEdges(p);
      if (ed) {
        if (r.crop !== "Manual") {
          r.manual_crop = (cropMargins ?? [0, 0, 0, 0]).map((v) => Math.max(0, v)) as any;
          r.crop = "Manual";
        }
        drag = { kind: "cropEdge", ...ed };
      } else drag = { kind: "cropNew", x: p.x, y: p.y };
    }
  }

  function onPointerMove(e: PointerEvent) {
    const p = local(e);
    hover = p;
    if (!drag || !rect) return;
    if (downAt && Math.hypot(p.x - downAt.x, p.y - downAt.y) > 3) moved = true;
    switch (drag.kind) {
      case "pan":
        vw.panX = drag.px + (p.x - drag.x);
        vw.panY = drag.py + (p.y - drag.y);
        break;
      case "grab": {
        const q = planeDirAt(p.x, p.y, drag.rc, drag.b);
        if (q) {
          const [y, pt] = solveGrab(drag.d0, q, r.yaw, r.pitch, r.roll);
          r.yaw = y;
          // Strip projections keep the horizon level unless Ctrl is held.
          if (!wraps(proj) || e.ctrlKey) r.pitch = pt;
        }
        break;
      }
      case "roll": {
        const a = Math.atan2(p.y - drag.cy, p.x - drag.cx);
        const sign = r.flip_h !== r.flip_v ? -1 : 1;
        let v = drag.roll0 - sign * ((a - drag.a0) * 180) / Math.PI;
        v = ((((v + 180) % 360) + 360) % 360) - 180;
        r.roll = v;
        break;
      }
      case "cropNew": {
        if (!moved) break;
        const x0 = Math.max(rect.x, Math.min(drag.x, p.x)), x1 = Math.min(rect.x + rect.w, Math.max(drag.x, p.x));
        const y0 = Math.max(rect.y, Math.min(drag.y, p.y)), y1 = Math.min(rect.y + rect.h, Math.max(drag.y, p.y));
        if (x1 - x0 > 6 && y1 - y0 > 6) {
          r.crop = "Manual";
          r.manual_crop = [(x0 - rect.x) / rect.w, (y0 - rect.y) / rect.h, (rect.x + rect.w - x1) / rect.w, (rect.y + rect.h - y1) / rect.h];
        }
        break;
      }
      case "cropEdge": {
        const m = [...r.manual_crop] as [number, number, number, number];
        const fx = Math.min(1, Math.max(0, (p.x - rect.x) / rect.w));
        const fy = Math.min(1, Math.max(0, (p.y - rect.y) / rect.h));
        if (drag.l) m[0] = Math.min(fx, 1 - m[2] - 0.02);
        if (drag.r) m[2] = Math.min(1 - fx, 1 - m[0] - 0.02);
        if (drag.t) m[1] = Math.min(fy, 1 - m[3] - 0.02);
        if (drag.b) m[3] = Math.min(1 - fy, 1 - m[1] - 0.02);
        r.manual_crop = m.map((v) => Math.max(0, Math.min(0.95, v))) as any;
        break;
      }
    }
  }

  function onPointerUp(e: PointerEvent) {
    const p = local(e);
    const d = drag;
    drag = null;
    if (d && (d.kind === "grab" || d.kind === "roll")) endInteraction();
    if (d && (d.kind === "cropNew" || d.kind === "cropEdge")) {
      commitHistory();
      persist();
    }
    if (!moved && e.button === 0) click(p);
    downAt = null;
  }

  function click(p: { x: number; y: number }) {
    const tool = vw.tool;
    if (tool === "center") {
      const d = screenToWorld(p.x, p.y);
      if (d) {
        const [y, pt] = yawPitchTowards(d);
        r.yaw = y;
        r.pitch = pt;
        viewChanged();
      }
    } else if (tool === "horizon" || tool === "vertical") {
      const d = screenToWorld(p.x, p.y);
      if (!d) return;
      points = [...points, d];
      vw.points = points.length;
      if (points.length === 2) {
        const res = level(r, points[0], points[1], tool === "horizon");
        points = [];
        vw.points = 0;
        if (res) {
          [r.yaw, r.pitch, r.roll] = res;
          viewChanged();
        }
      }
    }
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (!rect) return;
    const p = local(e);
    const factor = Math.exp(-e.deltaY * 0.0015);
    const nz = Math.min(40, Math.max(0.2, vw.zoom * factor));
    const f = nz / vw.zoom;
    // keep the point under the cursor fixed
    const cx = W / 2 + vw.panX, cy = H / 2 + vw.panY;
    vw.panX += (p.x - cx) * (1 - f);
    vw.panY += (p.y - cy) * (1 - f);
    vw.zoom = nz;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      points = [];
      vw.points = 0;
    }
  }

  const cursor = $derived.by(() => {
    if (drag?.kind === "grab" || drag?.kind === "pan") return "grabbing";
    if (drag?.kind === "roll") return "alias";
    switch (vw.tool) {
      case "rotate":
      case "pan":
        return "grab";
      case "crop": {
        if (hover) {
          const e = nearEdges(hover);
          if (e) return (e.l || e.r) && (e.t || e.b) ? ((e.l && e.t) || (e.r && e.b) ? "nwse-resize" : "nesw-resize") : e.l || e.r ? "ew-resize" : "ns-resize";
        }
        return "crosshair";
      }
      default:
        return "crosshair";
    }
  });

  const hoverInfo = $derived.by(() => {
    if (!hover) return "";
    const d = screenToWorld(hover.x, hover.y);
    if (!d) return "";
    const [lon, lat] = yawPitchTowards(d);
    return `${t("lon")} ${lon >= 0 ? "+" : ""}${lon.toFixed(1)}°   ${t("lat")} ${lat >= 0 ? "+" : ""}${lat.toFixed(1)}°`;
  });

  const hint = $derived.by(() => {
    switch (vw.tool) {
      case "rotate":
        return t("toolRotateHint");
      case "pan":
        return t("toolPanHint");
      case "center":
        return t("toolCenterHint");
      case "horizon":
        return points.length ? t("pickSecond") : t("pickFirstH");
      case "vertical":
        return points.length ? t("pickSecond") : t("pickFirstV");
      case "crop":
        return t("toolCropHint");
    }
  });

  const extent = $derived.by(() => {
    if (!L) return "";
    const ew = ((L.u1 - L.u0) * 180) / Math.PI;
    const eh = ((L.v1 - L.v0) * 180) / Math.PI;
    if (["Equirectangular", "Cylindrical", "Mercator", "Sinusoidal"].includes(L.projection)) return `${Math.min(360, ew).toFixed(0)}° × ${Math.min(180, eh).toFixed(0)}°`;
    return "";
  });

  onMount(() => {
    try {
      renderer = new SphereRenderer(canvas);
    } catch (e: any) {
      glError = String(e);
    }
    const ro = new ResizeObserver(() => {
      const b = host.getBoundingClientRect();
      dpr = window.devicePixelRatio || 1;
      W = b.width;
      H = b.height;
      canvas.width = Math.max(1, Math.round(b.width * dpr));
      canvas.height = Math.max(1, Math.round(b.height * dpr));
      schedule();
    });
    ro.observe(host);
    window.addEventListener("keydown", onKey);
    return () => {
      ro.disconnect();
      window.removeEventListener("keydown", onKey);
    };
  });

  const tools: { id: Tool; icon: string; label: () => string; hint: () => string; key: string }[] = [
    { id: "rotate", icon: "rotate3d", label: () => t("toolRotate"), hint: () => t("toolRotateHint"), key: "R" },
    { id: "pan", icon: "pan", label: () => t("toolPan"), hint: () => t("toolPanHint"), key: "H" },
    { id: "center", icon: "target", label: () => t("toolCenter"), hint: () => t("toolCenterHint"), key: "C" },
    { id: "horizon", icon: "horizon", label: () => t("toolHorizon"), hint: () => t("toolHorizonHint"), key: "L" },
    { id: "vertical", icon: "vertical", label: () => t("toolVertical"), hint: () => t("toolVerticalHint"), key: "V" },
    { id: "crop", icon: "crop", label: () => t("toolCrop"), hint: () => t("toolCropHint"), key: "K" },
  ];

  function rot(d: number) {
    r.roll = ((((r.roll + d + 180) % 360) + 360) % 360) - 180;
    viewChanged();
  }
</script>

<div class="viewer">
  <div class="toolbar">
    <div class="seg">
      {#each tools as tl}
        <button class:on={vw.tool === tl.id} data-tip="{tl.hint()} ({tl.key})" onclick={() => ((vw.tool = tl.id), (points = []))}>
          <Icon name={tl.icon} size={16} />
          <span class="tl">{tl.label()}</span>
        </button>
      {/each}
    </div>
    <div class="sep"></div>
    <button class="btn ghost icon" data-tip={t("rotL")} onclick={() => rot(90)}><Icon name="rotl" size={17} /></button>
    <button class="btn ghost icon" data-tip={t("rotR")} onclick={() => rot(-90)}><Icon name="rotr" size={17} /></button>
    <button class="btn ghost sm" data-tip={t("rot180")} onclick={() => rot(180)}>180°</button>
    <button class="btn ghost icon" class:active={r.flip_h} data-tip={t("flipH")} onclick={() => ((r.flip_h = !r.flip_h), viewChanged())}><Icon name="flipH" size={17} /></button>
    <button class="btn ghost icon" class:active={r.flip_v} data-tip={t("flipV")} onclick={() => ((r.flip_v = !r.flip_v), viewChanged())}><Icon name="flipV" size={17} /></button>
    <div class="sep"></div>
    <button class="btn ghost icon" data-tip="{t('undo')} (Ctrl+Z)" disabled={!canUndo()} onclick={undo}><Icon name="undo" size={17} /></button>
    <button class="btn ghost icon" data-tip="{t('redo')} (Ctrl+Y)" disabled={!canRedo()} onclick={redo}><Icon name="redo" size={17} /></button>
    <div class="grow"></div>
    <button class="btn ghost icon" class:active={S.settings.ui.showGrid} data-tip={t("gridHint")} onclick={() => ((S.settings.ui.showGrid = !S.settings.ui.showGrid), persist())}><Icon name="grid" size={17} /></button>
    <button class="btn ghost icon" class:active={S.settings.ui.showOutlines} data-tip={t("outlinesHint")} onclick={() => ((S.settings.ui.showOutlines = !S.settings.ui.showOutlines), persist())}><Icon name="outlines" size={17} /></button>
    <button class="btn ghost icon" class:active={S.settings.ui.showNav} data-tip={t("navPad")} onclick={() => ((S.settings.ui.showNav = !S.settings.ui.showNav), persist())}><Icon name="compass" size={17} /></button>
    <div class="sep"></div>
    <button class="btn ghost icon" data-tip={t("zoomOut")} onclick={() => (vw.zoom = Math.max(0.2, vw.zoom / 1.25))}><Icon name="zoomOut" size={17} /></button>
    <button class="btn ghost sm zoom" data-tip={t("fit")} onclick={fitView}>{Math.round(vw.zoom * 100)}%</button>
    <button class="btn ghost icon" data-tip={t("zoomIn")} onclick={() => (vw.zoom = Math.min(40, vw.zoom * 1.25))}><Icon name="zoomIn" size={17} /></button>
    <button class="btn ghost icon" data-tip="{t('fit')} (F)" onclick={fitView}><Icon name="fit" size={17} /></button>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    bind:this={host}
    style="cursor:{cursor}"
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointerleave={() => (hover = null)}
    onwheel={onWheel}
    ondblclick={() => (vw.tool === "rotate" || vw.tool === "pan") && fitView()}
    oncontextmenu={(e) => e.preventDefault()}
  >
    <canvas bind:this={canvas}></canvas>

    <svg class="ov" width={W} height={H}>
      {#if grid}
        {#each grid.lats as d}<path {d} class="g" />{/each}
        {#each grid.lons as d}<path {d} class="g" />{/each}
        <path d={grid.meridian} class="gm" />
        <path d={grid.horizon} class="gh" />
      {/if}
      {#each outlines as o}
        <path d={o.d} class="ol" class:sel={o.sel} />
        {#if o.label}
          <g transform="translate({o.label[0]},{o.label[1]})">
            <circle r="11" class="olc" class:sel={o.sel} />
            <text class="olt" text-anchor="middle" dy="4">{o.n}</text>
          </g>
        {/if}
      {/each}
      {#if cropRect && rect}
        <path
          class="shade"
          fill-rule="evenodd"
          d="M{rect.x},{rect.y}h{rect.w}v{rect.h}h{-rect.w}z M{cropRect.x},{cropRect.y}v{cropRect.h}h{cropRect.w}v{-cropRect.h}z"
        />
        <rect x={cropRect.x} y={cropRect.y} width={cropRect.w} height={cropRect.h} class="crop" class:edit={vw.tool === "crop"} />
        {#if vw.tool === "crop"}
          {#each [1, 2] as k}
            <line x1={cropRect.x + (cropRect.w * k) / 3} y1={cropRect.y} x2={cropRect.x + (cropRect.w * k) / 3} y2={cropRect.y + cropRect.h} class="third" />
            <line x1={cropRect.x} y1={cropRect.y + (cropRect.h * k) / 3} x2={cropRect.x + cropRect.w} y2={cropRect.y + (cropRect.h * k) / 3} class="third" />
          {/each}
          {#each [[0, 0], [1, 0], [0, 1], [1, 1], [0.5, 0], [0.5, 1], [0, 0.5], [1, 0.5]] as [fx, fy]}
            <rect x={cropRect.x + cropRect.w * fx - 5} y={cropRect.y + cropRect.h * fy - 5} width="10" height="10" rx="2" class="handle" />
          {/each}
        {/if}
      {/if}
      {#each points as pt}
        {@const s = worldToScreen(pt)}
        {#if s}
          <circle cx={s[0]} cy={s[1]} r="6" class="pt" />
          {#if hover}<line x1={s[0]} y1={s[1]} x2={hover.x} y2={hover.y} class="ptl" />{/if}
        {/if}
      {/each}
    </svg>

    {#if glError}
      <div class="center-msg err">WebGL: {glError}</div>
    {:else if app.layoutError}
      <div class="center-msg err">{app.layoutError}</div>
    {/if}

    {#if app.sphereBuilding}
      <div class="building">
        <span class="spinner"></span>
        {t("buildingPreview")}
        {#if app.job?.kind === "sphere"}<b>{Math.round(app.job.fraction * 100)}%</b>{/if}
      </div>
    {/if}

    {#if vw.interacting}
      <div class="hud-top">
        yaw <b>{r.yaw.toFixed(1)}°</b> · pitch <b>{r.pitch.toFixed(1)}°</b> · roll <b>{r.roll.toFixed(1)}°</b>
      </div>
    {/if}

    <div class="hud-bl">
      {#if hoverInfo}<span class="coord">{hoverInfo}</span>{/if}
      <span>{hint}</span>
    </div>
    <div class="hud-br">
      {#if extent}<span>{extent}</span>{/if}
      {#if L}<span>{L.outW} × {L.outH} px</span>{/if}
    </div>

    {#if S.settings.ui.showNav}
      <NavPad onstart={startInteraction} onend={endInteraction} />
    {/if}
  </div>
</div>

<style>
  .viewer {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 10px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    flex-wrap: nowrap;
    overflow: hidden;
  }
  .sep {
    width: 1px;
    height: 22px;
    background: var(--line-2);
    margin: 0 6px;
  }
  .grow {
    flex: 1;
  }
  .zoom {
    min-width: 54px;
    font-variant-numeric: tabular-nums;
  }
  .viewer {
    container-type: inline-size;
  }
  @container (max-width: 1050px) {
    .tl {
      display: none;
    }
  }
  .stage {
    position: relative;
    flex: 1;
    overflow: hidden;
    background: radial-gradient(ellipse at center, #171a21 0%, #111318 75%);
    touch-action: none;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .ov {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .g {
    fill: none;
    stroke: rgba(255, 255, 255, 0.16);
    stroke-width: 1;
  }
  .gm {
    fill: none;
    stroke: rgba(91, 155, 255, 0.55);
    stroke-width: 1;
  }
  .gh {
    fill: none;
    stroke: rgba(255, 170, 60, 0.85);
    stroke-width: 1.5;
  }
  .ol {
    fill: none;
    stroke: rgba(120, 180, 255, 0.85);
    stroke-width: 1.3;
  }
  .ol.sel {
    stroke: var(--accent);
    stroke-width: 2.6;
  }
  .olc {
    fill: rgba(10, 12, 16, 0.75);
    stroke: rgba(120, 180, 255, 0.85);
  }
  .olc.sel {
    stroke: var(--accent);
  }
  .olt {
    fill: #fff;
    font-size: 11px;
    font-weight: 600;
  }
  .shade {
    fill: rgba(5, 6, 9, 0.62);
  }
  .crop {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
  }
  .crop.edit {
    stroke-width: 2;
  }
  .third {
    stroke: rgba(255, 255, 255, 0.28);
    stroke-width: 1;
  }
  .handle {
    fill: var(--accent);
    stroke: #1b1306;
    stroke-width: 1;
  }
  .pt {
    fill: rgba(255, 170, 60, 0.25);
    stroke: var(--accent);
    stroke-width: 2;
  }
  .ptl {
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-dasharray: 5 4;
  }
  .center-msg {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    padding: 12px 18px;
    border-radius: var(--radius);
    background: rgba(20, 22, 28, 0.9);
    border: 1px solid var(--line-2);
    max-width: 70%;
    text-align: center;
  }
  .center-msg.err {
    color: #ffb3a8;
  }
  .building {
    position: absolute;
    top: 14px;
    right: 14px;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 12px;
    border-radius: 20px;
    background: rgba(20, 22, 28, 0.9);
    border: 1px solid var(--line-2);
    box-shadow: var(--shadow);
    font-size: 12px;
  }
  .spinner {
    width: 14px;
    height: 14px;
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
  .hud-top {
    position: absolute;
    top: 14px;
    left: 50%;
    transform: translateX(-50%);
    padding: 7px 14px;
    border-radius: 20px;
    background: rgba(12, 14, 18, 0.85);
    border: 1px solid var(--line-2);
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    pointer-events: none;
  }
  .hud-top b {
    color: var(--text);
  }
  .hud-bl,
  .hud-br {
    position: absolute;
    bottom: 12px;
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 6px 11px;
    border-radius: 8px;
    background: rgba(12, 14, 18, 0.72);
    color: var(--text-2);
    font-size: 12px;
    pointer-events: none;
    max-width: 60%;
  }
  .hud-bl {
    left: 12px;
  }
  .hud-br {
    right: 12px;
    font-variant-numeric: tabular-nums;
  }
  .coord {
    color: var(--text);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
