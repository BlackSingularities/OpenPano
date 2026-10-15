<script lang="ts">
  import Icon from "./Icon.svelte";
  import Section from "./Section.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";
  import Field from "./Field.svelte";
  import { t, projName, projDesc } from "../lib/i18n.svelte";
  import { FAMILIES, familyOf, kindOf, hasFov, fovRange, fovOfLayout, wrap180, type Family } from "../lib/composition";
  import type { Projection } from "../lib/geometry";
  import { S, app, vw, viewChanged, effectiveProjection, chooseProjection, lookOffsets, setLookOffsets, resetView } from "../lib/store.svelte";

  const r = $derived(S.settings.render);
  const proj = $derived(effectiveProjection());
  const fam = $derived(familyOf(proj));
  const kind = $derived(kindOf(proj));
  const off = $derived(lookOffsets());
  const variants = $derived(FAMILIES.find((f) => f.id === fam)?.projections ?? []);

  const ICON: Record<Family, string> = {
    sphere: "globe",
    strip: "pStrip",
    view: "pView",
    vertical: "pVertical",
    planet: "pPlanet",
    fisheye: "lens",
  };

  function pickFamily(f: Family) {
    const list = FAMILIES.find((x) => x.id === f)!.projections;
    // keep the current variant when re-clicking the active family
    chooseProjection(!r.auto_projection && list.includes(r.projection) ? r.projection : list[0]);
  }

  function useTool(tool: "center" | "horizon" | "vertical") {
    vw.tool = tool;
  }

  const fovAuto = $derived(r.fov <= 0);
  function setFovAuto(auto: boolean) {
    if (auto) r.fov = 0;
    else r.fov = app.layout ? fovOfLayout(proj, app.layout) : Math.round((fovRange(proj)[0] + fovRange(proj)[1]) / 2);
    viewChanged();
  }
  const tunnel = $derived(Math.abs(off.roll) > 90);
  // a tunnel is the planet turned upside down: its controls stay relative to that
  const planetTilt = $derived(tunnel ? -off.pitch : off.pitch);
</script>

<Section title={t("secCompose")} tip={t("h_secCompose")} icon="sphere" badge={app.align ? projName(proj) : ""}>
  <div class="step"><span class="num">1</span>{t("cStepType")}</div>
  <div class="fams">
    <button class="fc auto" class:on={r.auto_projection} onclick={() => chooseProjection("auto")}>
      <span class="fi"><Icon name="spark" size={18} /></span>
      <span class="ft">
        <b>{t("fam_auto")}</b>
        <small>{app.align ? `→ ${t(("fam_" + familyOf(app.align.autoProjection)) as any)} · ${projName(app.align.autoProjection)}` : t("famd_auto")}</small>
      </span>
    </button>
    {#each FAMILIES as f}
      <button class="fc" class:on={!r.auto_projection && fam === f.id} onclick={() => pickFamily(f.id)} data-tip={t(("famd_" + f.id) as any)}>
        <span class="fi"><Icon name={ICON[f.id]} size={18} /></span>
        <span class="ft">
          <b>{t(("fam_" + f.id) as any)}</b>
          <small>{t(("famd_" + f.id) as any)}</small>
        </span>
      </button>
    {/each}
  </div>
  {#if variants.length > 1}
    <Field label={t("cVariant")}>
      <div class="seg full">
        {#each variants as p}
          <button class:on={proj === p} data-tip={projDesc(p)} onclick={() => chooseProjection(p as Projection)}>{projName(p)}</button>
        {/each}
      </div>
    </Field>
  {/if}
  {#if proj === "Equirectangular"}
    <Switch label={t("fullSphere")} tip={t("h_fullSphere")} bind:checked={r.full_sphere} onchange={viewChanged} />
  {/if}

  <div class="step"><span class="num">2</span>{t("cStepAim")}</div>
  {#if kind === "strip"}
    <Slider label={t("cCentre")} tip={t("h_cCentre")} bind:value={r.yaw} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
    <div class="btnrow">
      <button class="btn sm" class:active={vw.tool === "center"} data-tip={t("toolCenterHint")} onclick={() => useTool("center")}><Icon name="target" size={15} />{t("cPickCentre")}</button>
    </div>
    <div class="sub" data-tip={t("h_cLevel")}>{t("cLevel")}</div>
    <div class="btnrow">
      <button class="btn sm" class:active={vw.tool === "horizon"} data-tip={t("toolHorizonHint")} onclick={() => useTool("horizon")}><Icon name="horizon" size={15} />{t("cByHorizon")}</button>
      <button class="btn sm" class:active={vw.tool === "vertical"} data-tip={t("toolVerticalHint")} onclick={() => useTool("vertical")}><Icon name="vertical" size={15} />{t("cByVertical")}</button>
    </div>
    <Slider label={t("cTilt")} tip={t("h_cTilt")} bind:value={r.pitch} min={-45} max={45} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
    <Slider label={t("cSlant")} tip={t("h_cSlant")} bind:value={r.roll} min={-45} max={45} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
    {#if r.pitch !== 0 || r.roll !== 0}
      <button class="btn sm ghost" onclick={() => ((r.pitch = 0), (r.roll = 0), viewChanged())}><Icon name="reset" size={15} />{t("cResetLevel")}</button>
    {/if}
  {:else if kind === "view"}
    <div class="sub">{t("cLook")}</div>
    <Slider label={t("cLookH")} tip={t("h_cLookH")} bind:value={r.yaw} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
    <Slider label={t("cLookV")} tip={t("h_cLookV")} value={off.pitch} min={-90} max={90} step={0.1} decimals={1} unit="°" onchange={(v) => (setLookOffsets(v, null), viewChanged())} />
    <Slider label={t("cFrameRot")} tip={t("h_cFrameRot")} value={off.roll} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={(v) => (setLookOffsets(null, v), viewChanged())} />
    <div class="btnrow">
      <button class="btn sm" data-tip={t("rotL")} onclick={() => (setLookOffsets(null, wrap180(off.roll + 90)), viewChanged())}><Icon name="rotl" size={15} />90°</button>
      <button class="btn sm" data-tip={t("rotR")} onclick={() => (setLookOffsets(null, wrap180(off.roll - 90)), viewChanged())}><Icon name="rotr" size={15} />90°</button>
      <button class="btn sm" class:active={vw.tool === "center"} data-tip={t("toolCenterHint")} onclick={() => useTool("center")}><Icon name="target" size={15} />{t("cPickCentre")}</button>
    </div>
    {#if hasFov(proj)}
      <Field label={t("cFov")} tip={t("h_cFov")}>
        <div class="seg">
          <button class:on={fovAuto} onclick={() => setFovAuto(true)}>{t("auto")}</button>
          <button class:on={!fovAuto} onclick={() => setFovAuto(false)}>{t("fovManual")}</button>
        </div>
      </Field>
      {#if !fovAuto}
        <Slider label={t("cFov")} bind:value={r.fov} min={fovRange(proj)[0]} max={fovRange(proj)[1]} step={1} unit="°" oncommit={() => viewChanged()} />
      {/if}
    {/if}
    <div class="sub" data-tip={t("h_cLevel")}>{t("cLevel")}</div>
    <div class="btnrow">
      <button class="btn sm" class:active={vw.tool === "horizon"} data-tip={t("toolHorizonHint")} onclick={() => useTool("horizon")}><Icon name="horizon" size={15} />{t("cByHorizon")}</button>
      <button class="btn sm" class:active={vw.tool === "vertical"} data-tip={t("toolVerticalHint")} onclick={() => useTool("vertical")}><Icon name="vertical" size={15} />{t("cByVertical")}</button>
    </div>
  {:else}
    <Slider label={t("cPlanetSpin")} tip={t("h_cPlanetSpin")} bind:value={r.yaw} min={-180} max={180} step={0.1} decimals={1} unit="°" onchange={() => viewChanged()} />
    <Slider label={t("cPlanetTilt")} tip={t("h_cPlanetTilt")} value={planetTilt} min={-60} max={60} step={0.1} decimals={1} unit="°" onchange={(v) => (setLookOffsets(tunnel ? -v : v, null), viewChanged())} />
    <Field label={t("cPlanetSize")} tip={t("h_cPlanetSize")}>
      <div class="seg">
        <button class:on={fovAuto} onclick={() => setFovAuto(true)}>{t("auto")}</button>
        <button class:on={!fovAuto} onclick={() => setFovAuto(false)}>{t("fovManual")}</button>
      </div>
    </Field>
    {#if !fovAuto}
      <Slider label={t("cPlanetSize")} bind:value={r.fov} min={fovRange(proj)[0]} max={fovRange(proj)[1]} step={1} unit="°" oncommit={() => viewChanged()} />
    {/if}
    <Switch label={t("cTunnel")} tip={t("h_cTunnel")} checked={tunnel} onchange={(v) => (setLookOffsets(-off.pitch, v ? 180 : 0), viewChanged())} />
    <div class="sub" data-tip={t("h_cLevel")}>{t("cLevel")}</div>
    <div class="btnrow">
      <button class="btn sm" class:active={vw.tool === "horizon"} data-tip={t("toolHorizonHint")} onclick={() => useTool("horizon")}><Icon name="horizon" size={15} />{t("cByHorizon")}</button>
      <button class="btn sm" class:active={vw.tool === "vertical"} data-tip={t("toolVerticalHint")} onclick={() => useTool("vertical")}><Icon name="vertical" size={15} />{t("cByVertical")}</button>
    </div>
  {/if}
  <div class="btnrow end">
    <button class="btn sm icon" class:active={r.flip_h} data-tip={t("flipH")} onclick={() => ((r.flip_h = !r.flip_h), viewChanged())}><Icon name="flipH" size={15} /></button>
    <button class="btn sm icon" class:active={r.flip_v} data-tip={t("flipV")} onclick={() => ((r.flip_v = !r.flip_v), viewChanged())}><Icon name="flipV" size={15} /></button>
    <span class="grow"></span>
    <button class="btn sm ghost" data-tip={t("h_resetView")} onclick={resetView}><Icon name="reset" size={15} />{t("resetView")}</button>
  </div>
</Section>

<style>
  .step {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-2);
    font-weight: 600;
    margin-top: 2px;
  }
  .num {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--sel-bg);
    color: var(--sel-fg);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10.5px;
  }
  .sub {
    font-size: 11.5px;
    color: var(--text-3);
    margin-top: 2px;
    cursor: help;
  }
  .fams {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 5px;
  }
  .fc {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--surface);
    cursor: pointer;
    text-align: left;
    min-width: 0;
    color: var(--text);
  }
  .fc:hover {
    border-color: var(--line-2);
    background: var(--raised);
  }
  .fc.on {
    border-color: var(--sel-bg);
    background: var(--sel-bg);
    color: var(--sel-fg);
  }
  .fc.auto {
    grid-column: 1 / -1;
  }
  .fi {
    width: 30px;
    height: 30px;
    flex: none;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--raised);
  }
  .fc.on .fi {
    background: rgba(var(--bg-rgb), 0.12);
  }
  .ft {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .ft b {
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ft small {
    font-size: 10.5px;
    line-height: 1.25;
    color: var(--text-3);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .fc.on small {
    color: var(--sel-fg);
    opacity: 0.7;
  }
  .btnrow {
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
    align-items: center;
  }
  .btnrow.end {
    margin-top: 2px;
  }
  .grow {
    flex: 1;
  }
</style>
