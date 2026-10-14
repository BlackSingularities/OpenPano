<script lang="ts">
  import Icon from "./Icon.svelte";
  import Section from "./Section.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";
  import Field from "./Field.svelte";
  import { t, projName, projDesc, lensName } from "../lib/i18n.svelte";
  import { PROJECTIONS, type LensType, type Projection } from "../lib/geometry";
  import { S, app, vw, align, busyBlocking, readyIds, persist, viewChanged, buildSphere, setPhotometric, renderAndSave, resolvedFormat, anyRaw, effectiveProjection, resetView, commitHistory, nudgeView } from "../lib/store.svelte";

  const s = $derived(S.settings);
  const r = $derived(S.settings.render);
  const lenses: LensType[] = ["Rectilinear", "FisheyeEquidistant", "FisheyeEquisolid", "FisheyeStereographic"];

  const focalLabel = $derived.by(() => {
    switch (app.align?.focalSource) {
      case "manual":
        return t("focalManual");
      case "exif":
        return t("focalExif");
      case "estimated":
        return t("focalEstimated");
      default:
        return t("focalDefault");
    }
  });
  const rmsClass = $derived(!app.align ? "" : app.align.rms < 2 ? "good" : app.align.rms < 5 ? "mid" : "bad");
  const alignedCount = $derived(app.align ? app.align.cameras.filter((c) => c.aligned).length : 0);

  function setProjection(p: Projection | "auto") {
    if (p === "auto") r.auto_projection = true;
    else {
      r.auto_projection = false;
      r.projection = p;
    }
    viewChanged();
  }

  const fmt = $derived(resolvedFormat());
  const outSize = $derived.by(() => {
    const L = app.layout;
    if (!L) return null;
    let m: [number, number, number, number] = [0, 0, 0, 0];
    if (r.crop === "Manual") m = r.manual_crop;
    else if (r.crop === "Auto" && app.autoCrop) m = app.autoCrop;
    const w = Math.max(1, Math.round(L.outW * (1 - m[0] - m[2])));
    const h = Math.max(1, Math.round(L.outH * (1 - m[1] - m[3])));
    return { w, h, mp: (w * h) / 1e6, ram: ((L.outW * L.outH) / 1e6) * 40 / 1024 };
  });

  function rebuild() {
    persist();
    buildSphere();
  }
</script>

<aside class="insp">
  <div class="scroll">
    <Section title={t("secLens")} tip={t("h_secLens")} icon="lens" badge={app.align ? `${alignedCount}/${app.align.cameras.length}` : ""}>
      <Field label={t("lens")} tip={t("h_lens")}>
        <select bind:value={s.align.lens} onchange={persist}>
          {#each lenses as l}<option value={l}>{lensName(l)}</option>{/each}
        </select>
      </Field>
      <Field label={t("fov")} tip={t("h_fov")}>
        <div class="seg">
          <button class:on={!s.manualFov} onclick={() => ((s.manualFov = false), persist())}>{t("auto")}</button>
          <button class:on={s.manualFov} onclick={() => ((s.manualFov = true), persist())}>{t("fovManual")}</button>
        </div>
        {#if s.manualFov}
          <input type="number" min="5" max="360" step="0.5" bind:value={s.fovDeg} onchange={persist} /><span class="muted">°</span>
        {/if}
      </Field>
      <Slider label={t("features")} tip={t("h_features")} bind:value={s.align.max_features} min={500} max={8000} step={250} oncommit={persist} />
      <Field label={t("matching")} tip={t("h_matching")}>
        <select bind:value={s.align.strategy} onchange={persist}>
          <option value="Auto">{t("stratAuto")}</option>
          <option value="AllPairs">{t("stratAll")}</option>
          <option value="Sequential">{t("stratSeq")}</option>
        </select>
      </Field>
      {#if s.align.strategy !== "AllPairs"}
        <Slider label={t("neighbours")} tip={t("h_neighbours")} bind:value={s.align.window} min={1} max={10} oncommit={persist} />
      {/if}
      <Slider label={t("ransac")} tip={t("h_ransac")} bind:value={s.align.ransac_threshold_px} min={1} max={10} step={0.5} decimals={1} unit="px" oncommit={persist} />
      <Switch label={t("optimizeFocal")} tip={t("h_optimizeFocal")} bind:checked={s.align.optimize_focal} onchange={persist} />
      <Switch label={t("straighten")} tip={t("h_straighten")} bind:checked={s.align.straighten} onchange={persist} />
      <button data-tip={t("h_align")} class="btn blue lg wide" disabled={busyBlocking() || readyIds().length < 2} onclick={align}>
        <Icon name="align" size={17} />{app.align ? t("realign") : t("alignImages")}
      </button>
      {#if app.align}
        <div class="stats">
          <div data-tip={t("h_statImages")}><span>{t("statImages")}</span><b>{alignedCount}/{app.align.cameras.length}</b></div>
          <div data-tip={t("h_statError")}><span>{t("statError")}</span><b class={rmsClass}>{app.align.rms.toFixed(2)} px</b></div>
          <div data-tip={t("h_statFov")}><span>{t("statFov")}</span><b>{app.align.hfov.toFixed(1)}° <em>({focalLabel})</em></b></div>
          <div data-tip={t("h_statCoverage")}><span>{t("statCoverage")}</span><b>{app.align.coverage[0].toFixed(0)}° × {(app.align.coverage[2] - app.align.coverage[1]).toFixed(0)}°</b></div>
          <div data-tip={t("h_statPairs")}><span>{t("statPairs")}</span><b>{app.align.pairs}</b></div>
        </div>
      {/if}
    </Section>

    <Section title={t("secProjection")} tip={t("h_secProjection")} icon="sphere" badge={app.align ? projName(effectiveProjection()) : ""}>
      <div class="projs">
        <button class="pc auto" class:on={r.auto_projection} onclick={() => setProjection("auto")} data-tip={t("projAutoDesc")}>
          <span class="pn"><Icon name="spark" size={14} />{t("projAuto")}</span>
          <span class="pd">{app.align ? "→ " + projName(app.align.autoProjection) : t("projAutoDesc")}</span>
        </button>
        {#each PROJECTIONS as p}
          <button class="pc" class:on={!r.auto_projection && r.projection === p} onclick={() => setProjection(p)} data-tip={projDesc(p)}>
            <span class="pn">{projName(p)}</span>
          </button>
        {/each}
      </div>
      <p class="desc">{r.auto_projection ? t("projAutoDesc") : projDesc(r.projection)}</p>
      <Switch label={t("fullSphere")} tip={t("h_fullSphere")} disabled={effectiveProjection() !== "Equirectangular"} bind:checked={r.full_sphere} onchange={viewChanged} />
      <Switch label={t("circularFisheye")} tip={t("h_circularFisheye")} disabled={s.align.lens === "Rectilinear"} bind:checked={r.circular_fisheye} onchange={rebuild} />
    </Section>

    <Section title={t("secView")} tip={t("h_secView")} icon="compass">
      <Slider label={t("yaw")} tip={t("h_yaw")} bind:value={r.yaw} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
      <Slider label={t("pitch")} tip={t("h_pitch")} bind:value={r.pitch} min={-90} max={90} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
      <Slider label={t("roll")} tip={t("h_roll")} bind:value={r.roll} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
      <div class="btnrow">
        <button class="btn sm" data-tip={t("rotL")} onclick={() => (nudgeView(0, 0, 90), viewChanged())}><Icon name="rotl" size={15} />90°</button>
        <button class="btn sm" data-tip={t("rotR")} onclick={() => (nudgeView(0, 0, -90), viewChanged())}><Icon name="rotr" size={15} />90°</button>
        <button class="btn sm" data-tip={t("rot180")} onclick={() => (nudgeView(0, 0, 180), viewChanged())}>180°</button>
        <button class="btn sm icon" class:active={r.flip_h} data-tip={t("flipH")} onclick={() => ((r.flip_h = !r.flip_h), viewChanged())}><Icon name="flipH" size={15} /></button>
        <button class="btn sm icon" class:active={r.flip_v} data-tip={t("flipV")} onclick={() => ((r.flip_v = !r.flip_v), viewChanged())}><Icon name="flipV" size={15} /></button>
      </div>
      <div class="btnrow">
        <button class="btn sm" class:active={vw.tool === "center"} data-tip={t("toolCenterHint")} onclick={() => (vw.tool = "center")}><Icon name="target" size={15} />{t("toolCenter")}</button>
        <button class="btn sm" class:active={vw.tool === "horizon"} data-tip={t("toolHorizonHint")} onclick={() => (vw.tool = "horizon")}><Icon name="horizon" size={15} />{t("toolHorizon")}</button>
        <button class="btn sm" class:active={vw.tool === "vertical"} data-tip={t("toolVerticalHint")} onclick={() => (vw.tool = "vertical")}><Icon name="vertical" size={15} />{t("toolVertical")}</button>
      </div>
      <button class="btn sm ghost" data-tip={t("h_resetView")} onclick={resetView}><Icon name="reset" size={15} />{t("resetView")}</button>
    </Section>

    <Section title={t("secBlend")} tip={t("h_secBlend")} icon="layers" open={false}>
      <Field label={t("blend")} tip={t("h_blend")}>
        <div class="seg full">
          <button data-tip={t("h_blendMulti")} class:on={r.blend === "Multiband"} onclick={() => ((r.blend = "Multiband"), rebuild())}>{t("blendMulti")}</button>
          <button data-tip={t("h_blendFeather")} class:on={r.blend === "Feather"} onclick={() => ((r.blend = "Feather"), rebuild())}>{t("blendFeather")}</button>
          <button data-tip={t("h_blendSeams")} class:on={r.blend === "Seams"} onclick={() => ((r.blend = "Seams"), rebuild())}>{t("blendSeams")}</button>
        </div>
      </Field>
      {#if r.blend === "Multiband"}
        <Slider label={t("bands")} tip={t("h_bands")} bind:value={r.bands} min={0} max={10} format={(v) => (v === 0 ? t("auto") : String(v))} oncommit={rebuild} />
      {/if}
      <Switch label={t("exposure")} tip={t("h_exposure")} bind:checked={s.photo.exposure} onchange={setPhotometric} />
      <Switch label={t("color")} tip={t("h_color")} bind:checked={s.photo.color} onchange={setPhotometric} />
      <Switch label={t("vignetting")} tip={t("h_vignetting")} bind:checked={s.photo.vignetting} onchange={setPhotometric} />
      <Switch label={t("bicubic")} tip={t("h_bicubic")} bind:checked={r.bicubic} onchange={persist} />
    </Section>

    <Section title={t("secCrop")} tip={t("h_secCrop")} icon="crop">
      <div class="seg full">
        <button data-tip={t("h_cropAuto")} class:on={r.crop === "Auto"} onclick={() => ((r.crop = "Auto"), commitHistory(), persist())}>{t("cropAuto")}</button>
        <button data-tip={t("h_cropNone")} class:on={r.crop === "None"} onclick={() => ((r.crop = "None"), commitHistory(), persist())}>{t("cropNone")}</button>
        <button
          data-tip={t("h_cropManual")}
          class:on={r.crop === "Manual"}
          onclick={() => {
            if (r.crop !== "Manual" && app.autoCrop) r.manual_crop = [...app.autoCrop] as any;
            r.crop = "Manual";
            commitHistory();
            persist();
          }}>{t("cropManual")}</button
        >
      </div>
      <button class="btn sm" data-tip={t("toolCropHint")} class:active={vw.tool === "crop"} onclick={() => (vw.tool = "crop")}><Icon name="crop" size={15} />{t("cropTool")}</button>
      {#if r.crop === "Manual"}
        {#each [t("left"), t("top"), t("right"), t("bottom")] as lbl, k}
          <Slider
            label={lbl}
            tip={t("h_cropMargin")}
            value={r.manual_crop[k] * 100}
            min={0}
            max={45}
            step={0.5}
            decimals={1}
            unit="%"
            onchange={(v) => {
              const m = [...r.manual_crop] as [number, number, number, number];
              m[k] = v / 100;
              r.manual_crop = m;
            }}
            oncommit={() => (commitHistory(), persist())}
          />
        {/each}
      {/if}
    </Section>

    <Section title={t("secOutput")} tip={t("h_secOutput")} icon="download">
      <Field label={t("format")} tip={t("h_format")}>
        <select bind:value={s.output.format} onchange={persist}>
          <option value="Auto">{t("formatAuto")} ({anyRaw() ? "DNG" : "JPEG"})</option>
          <option value="Jpeg">JPEG · 8-bit sRGB</option>
          <option value="Tiff16">TIFF · 16-bit sRGB</option>
          <option value="Png16">PNG · 16-bit sRGB</option>
          <option value="Dng">{t("dngDesc")}</option>
        </select>
      </Field>
      {#if fmt === "Jpeg"}
        <Slider label={t("quality")} tip={t("h_quality")} bind:value={s.output.jpeg_quality} min={50} max={100} oncommit={persist} />
      {/if}
      <Slider label={t("resolution")} tip={t("h_resolution")} value={r.scale * 100} min={10} max={150} step={5} unit="%" onchange={(v) => (r.scale = v / 100)} oncommit={() => (persist(), viewChanged())} />
      <Field label={t("maxMp")} tip={t("h_maxMp")}>
        <input type="number" min="1" max="2000" bind:value={r.max_megapixels} onchange={() => (persist(), viewChanged())} /><span class="muted">MP</span>
      </Field>
      {#if fmt === "Png16" || fmt === "Tiff16"}
        <Switch label={t("transparent")} tip={t("h_transparent")} bind:checked={s.output.transparent} onchange={persist} />
      {/if}
      {#if effectiveProjection() === "Equirectangular"}
        <Switch label={t("gpano")} tip={t("h_gpano")} bind:checked={s.output.gpano} onchange={persist} />
      {/if}
      <Switch label={t("revealAfter")} tip={t("h_revealAfter")} bind:checked={s.ui.revealAfter} onchange={persist} />
      {#if outSize}
        <div class="out" data-tip={t("h_outSize")}>
          <div><span>{t("outputSize")}</span><b>{outSize.w} × {outSize.h}</b></div>
          <div><span>MP</span><b>{outSize.mp.toFixed(1)}</b></div>
          <div><span>RAM</span><b>~{outSize.ram.toFixed(1)} GB</b></div>
        </div>
      {/if}
      <button data-tip={t("h_render")} class="btn primary lg wide" disabled={!app.align || busyBlocking()} onclick={renderAndSave}>
        <Icon name="download" size={17} />{t("renderSave")} · {fmt === "Dng" ? "DNG" : fmt === "Jpeg" ? "JPEG" : fmt === "Tiff16" ? "TIFF" : "PNG"}
      </button>
    </Section>
  </div>
</aside>

<style>
  .insp {
    height: 100%;
    background: var(--panel);
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 10px 10px 16px;
  }
  .stats,
  .out {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px 14px;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--line);
  }
  .out {
    grid-template-columns: 1.4fr 0.8fr 0.9fr;
  }
  .stats div,
  .out div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .stats span,
  .out span {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-3);
  }
  .stats b,
  .out b {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stats em {
    font-style: normal;
    font-weight: 400;
    color: var(--text-3);
  }
  .good {
    color: var(--ok);
  }
  .mid {
    color: var(--warn);
  }
  .bad {
    color: var(--err);
  }
  .projs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 5px;
  }
  .pc {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    padding: 7px 9px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-2);
    cursor: pointer;
    text-align: left;
    min-width: 0;
  }
  .pc:hover {
    border-color: var(--line-2);
    background: var(--raised);
  }
  .pc.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .pc.auto {
    grid-column: 1 / -1;
  }
  .pn {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .pc.auto .pn :global(svg) {
    color: var(--accent);
  }
  .pd {
    font-size: 11px;
    color: var(--text-3);
  }
  .desc {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .btnrow {
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
  }
</style>
