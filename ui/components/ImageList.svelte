<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { yawPitchTowards, mul } from "../lib/geometry";
  import { S, app, pickFiles, pickFolder, removeImage, toggleImage, setAllEnabled, sortImages, clearImages, reveal, busyBlocking, viewChanged } from "../lib/store.svelte";

  let menu = $state<{ id: number; x: number; y: number } | null>(null);

  const enabled = $derived(app.images.filter((x) => x.enabled).length);

  function camOf(id: number) {
    const idx = app.align?.cameras.findIndex((c) => c.id === id) ?? -1;
    return idx >= 0 ? { idx, cam: app.align!.cameras[idx] } : null;
  }

  function centerOn(id: number) {
    const c = camOf(id);
    if (!c || !c.cam.aligned) return;
    const [y, p] = yawPitchTowards(mul(c.cam.rot, [0, 0, 1]));
    S.settings.render.yaw = y;
    S.settings.render.pitch = p;
    viewChanged();
  }

  function openMenu(e: MouseEvent, id: number) {
    e.preventDefault();
    menu = { id, x: Math.min(e.clientX, window.innerWidth - 240), y: Math.min(e.clientY, window.innerHeight - 190) };
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<aside class="images">
  <div class="head">
    <div class="title">
      <Icon name="image" size={16} />
      <span>{t("images")}</span>
      {#if app.images.length}<span class="count">{enabled}/{app.images.length}</span>{/if}
    </div>
    <div class="acts">
      <button class="btn ghost sm icon" data-tip={t("addImages")} onclick={pickFiles}><Icon name="plus" size={16} /></button>
      <button class="btn ghost sm icon" data-tip={t("addFolder")} onclick={pickFolder}><Icon name="folder" size={16} /></button>
    </div>
  </div>
  {#if app.images.length}
    <div class="bar">
      <button class="chip" disabled={busyBlocking()} onclick={() => setAllEnabled(true)}>{t("selectAll")}</button>
      <button class="chip" disabled={busyBlocking()} onclick={() => setAllEnabled(false)}>{t("selectNone")}</button>
      <button class="chip" disabled={busyBlocking()} onclick={sortImages}><Icon name="sort" size={12} />{t("sort")}</button>
      <span class="grow"></span>
      <button class="chip danger" disabled={busyBlocking()} onclick={clearImages}><Icon name="trash" size={12} />{t("clear")}</button>
    </div>
  {/if}

  <div class="list">
    {#if !app.images.length}
      <button class="empty" onclick={pickFiles}>
        <span class="eicon"><Icon name="image" size={30} stroke={1.4} /></span>
        <b>{t("dropHint")}</b>
        <span class="muted small">{t("formats")}</span>
      </button>
    {/if}
    {#each app.images as it (it.id)}
      {@const c = camOf(it.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="card"
        class:sel={app.selectedId === it.id}
        class:off={!it.enabled}
        onclick={() => (app.selectedId = app.selectedId === it.id ? null : it.id)}
        ondblclick={() => centerOn(it.id)}
        oncontextmenu={(e) => openMenu(e, it.id)}
      >
        <div class="thumb">
          {#if it.thumb}
            <img src={it.thumb} alt="" draggable="false" />
          {:else if it.status === "loading"}
            <span class="spin"></span>
          {:else}
            <Icon name="alert" size={20} />
          {/if}
          {#if c}<span class="num" class:bad={!c.cam.aligned}>{c.idx + 1}</span>{/if}
          {#if it.isRaw}<span class="raw">RAW</span>{/if}
        </div>
        <div class="meta">
          <div class="name" data-tip={it.path}>{it.name}</div>
          {#if it.status === "ok"}
            <div class="sub">{it.width} × {it.height}{it.camera ? " · " + it.camera : ""}</div>
            {#if it.summary}<div class="sub">{it.summary}</div>{/if}
          {:else if it.status === "loading"}
            <div class="sub">{t("loading")}</div>
          {:else}
            <div class="sub err" data-tip={it.error}>{it.error}</div>
          {/if}
          {#if c}
            {#if c.cam.aligned}
              <div class="st ok"><Icon name="check" size={12} stroke={2.4} />{t("aligned")}</div>
            {:else}
              <div class="st warn"><Icon name="alert" size={12} />{t("notConnected")}</div>
            {/if}
          {:else if !it.enabled}
            <div class="st">{t("disabled")}</div>
          {/if}
        </div>
        <button
          class="check"
          class:on={it.enabled}
          data-tip={t("useImage")}
          disabled={busyBlocking()}
          onclick={(e) => {
            e.stopPropagation();
            toggleImage(it.id);
          }}
        >
          {#if it.enabled}<Icon name="check" size={13} stroke={2.6} />{/if}
        </button>
      </div>
    {/each}
  </div>
</aside>

{#if menu}
  {@const it = app.images.find((x) => x.id === menu!.id)}
  {@const c = it ? camOf(it.id) : null}
  {#if it}
    <div class="menu" style="left:{menu.x}px;top:{menu.y}px">
      <button disabled={!c?.cam.aligned} onclick={() => centerOn(it.id)}><Icon name="target" size={15} />{t("centerOnImage")}</button>
      <button disabled={busyBlocking()} onclick={() => toggleImage(it.id)}><Icon name={it.enabled ? "x" : "check"} size={15} />{it.enabled ? t("disable") : t("enable")}</button>
      <button onclick={() => reveal(it.path)}><Icon name="folder" size={15} />{t("reveal")}</button>
      <div class="msep"></div>
      <button class="danger" disabled={busyBlocking()} onclick={() => removeImage(it.id)}><Icon name="trash" size={15} />{t("remove")}</button>
    </div>
  {/if}
{/if}

<style>
  .images {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel);
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 12px 8px;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 14px;
  }
  .title :global(svg) {
    color: var(--accent);
  }
  .count {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-2);
    background: var(--raised);
    padding: 2px 8px;
    border-radius: 20px;
  }
  .acts {
    display: flex;
    gap: 2px;
  }
  .bar {
    display: flex;
    gap: 5px;
    padding: 0 12px 10px;
    align-items: center;
  }
  .grow {
    flex: 1;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 9px;
    border-radius: 20px;
    border: 1px solid var(--line-2);
    background: transparent;
    color: var(--text-2);
    font-size: 11.5px;
    cursor: pointer;
  }
  .chip:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }
  .chip:disabled {
    opacity: 0.4;
  }
  .chip.danger:hover:not(:disabled) {
    color: var(--err);
    border-color: rgba(255, 107, 107, 0.5);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .empty {
    margin: 8px 4px;
    padding: 34px 16px;
    border: 1.5px dashed var(--line-2);
    border-radius: var(--radius);
    background: rgba(255, 255, 255, 0.015);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    cursor: pointer;
    color: var(--text-2);
  }
  .empty:hover {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .eicon {
    color: var(--accent);
  }
  .card {
    position: relative;
    display: flex;
    gap: 10px;
    padding: 7px;
    border-radius: var(--radius);
    background: var(--panel-2);
    border: 1px solid transparent;
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s;
  }
  .card:hover {
    background: var(--raised);
  }
  .card.sel {
    border-color: var(--accent);
    background: #2a2a2a;
    background: linear-gradient(0deg, var(--accent-soft), var(--accent-soft)), var(--panel-2);
  }
  .card.off .thumb,
  .card.off .name {
    opacity: 0.45;
  }
  .thumb {
    position: relative;
    width: 92px;
    height: 62px;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
    background: var(--bg-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--err);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .num {
    position: absolute;
    left: 4px;
    top: 4px;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: rgba(10, 12, 16, 0.82);
    color: var(--accent-2);
    font-size: 10.5px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .num.bad {
    color: var(--warn);
  }
  .raw {
    position: absolute;
    right: 4px;
    bottom: 4px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 9.5px;
    font-weight: 800;
    letter-spacing: 0.4px;
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-top: 1px;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-right: 22px;
  }
  .sub {
    font-size: 11px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub.err {
    color: var(--err);
  }
  .st {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-3);
    margin-top: 1px;
  }
  .st.ok {
    color: var(--ok);
  }
  .st.warn {
    color: var(--warn);
  }
  .check {
    position: absolute;
    right: 8px;
    top: 8px;
    width: 18px;
    height: 18px;
    border-radius: 5px;
    border: 1.5px solid var(--line-2);
    background: var(--bg-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent-ink);
    cursor: pointer;
    padding: 0;
  }
  .check.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .spin {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.12);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 230px;
    padding: 5px;
    border-radius: 10px;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    box-shadow: var(--shadow);
  }
  .menu button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 9px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    text-align: left;
  }
  .menu button:hover:not(:disabled) {
    background: var(--raised-2);
  }
  .menu button:disabled {
    opacity: 0.4;
  }
  .menu button.danger:hover:not(:disabled) {
    color: var(--err);
  }
  .msep {
    height: 1px;
    background: var(--line);
    margin: 4px 2px;
  }
</style>
