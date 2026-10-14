<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { vw, nudgeView, resetView, fitView } from "../lib/store.svelte";

  let { onstart, onend }: { onstart: () => void; onend: () => void } = $props();

  let held: { dy: number; dp: number; dr: number } | null = null;
  let last = 0;
  let raf = 0;
  let fast = false;
  let active = $state<string | null>(null);

  function loop(ts: number) {
    if (!held) return;
    const dt = last ? Math.min(0.05, (ts - last) / 1000) : 0;
    last = ts;
    const speed = fast ? 120 : 40; // deg/s
    nudgeView(held.dy * speed * dt, held.dp * speed * dt, held.dr * speed * dt);
    raf = requestAnimationFrame(loop);
  }

  function down(e: PointerEvent, id: string, dy: number, dp: number, dr: number) {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    fast = e.shiftKey;
    held = { dy, dp, dr };
    active = id;
    last = 0;
    onstart();
    // immediate small step so a click always does something
    nudgeView(dy * 0.5, dp * 0.5, dr * 0.5);
    raf = requestAnimationFrame(loop);
  }
  function up() {
    if (!held) return;
    held = null;
    active = null;
    cancelAnimationFrame(raf);
    onend();
  }

  const B = [
    { id: "rl", icon: "rotl", tip: () => t("navRollL"), d: [0, 0, 1] },
    { id: "u", icon: "up", tip: () => t("navUp"), d: [0, 1, 0] },
    { id: "rr", icon: "rotr", tip: () => t("navRollR"), d: [0, 0, -1] },
    { id: "l", icon: "left", tip: () => t("navLeft"), d: [-1, 0, 0] },
    { id: "c", icon: "reset", tip: () => t("resetView"), d: null },
    { id: "r", icon: "right", tip: () => t("navRight"), d: [1, 0, 0] },
    { id: "zo", icon: "zoomOut", tip: () => t("zoomOut"), d: null },
    { id: "dn", icon: "down", tip: () => t("navDown"), d: [0, -1, 0] },
    { id: "zi", icon: "zoomIn", tip: () => t("zoomIn"), d: null },
  ] as const;

  function click(id: string) {
    if (id === "c") resetView();
    else if (id === "zo") vw.zoom = Math.max(0.2, vw.zoom / 1.25);
    else if (id === "zi") vw.zoom = Math.min(40, vw.zoom * 1.25);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pad" onpointerdown={(e) => e.stopPropagation()} onwheel={(e) => e.stopPropagation()} data-tip={t("holdHint")}>
  <div class="hdr">{t("navigate")}</div>
  <div class="grid">
    {#each B as b}
      {#if b.d}
        <button
          class="nb"
          class:on={active === b.id}
          data-tip={b.tip()}
          onpointerdown={(e) => down(e, b.id, b.d[0], b.d[1], b.d[2])}
          onpointerup={up}
          onpointercancel={up}
          onlostpointercapture={up}><Icon name={b.icon} size={18} /></button
        >
      {:else}
        <button class="nb sec" data-tip={b.tip()} onclick={() => click(b.id)}><Icon name={b.icon} size={17} /></button>
      {/if}
    {/each}
  </div>
  <button class="fitb" onclick={fitView}><Icon name="fit" size={14} />{t("fit")}</button>
</div>

<style>
  .pad {
    position: absolute;
    right: 14px;
    bottom: 54px;
    padding: 9px;
    border-radius: 14px;
    background: rgba(22, 25, 32, 0.88);
    backdrop-filter: blur(10px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: var(--shadow);
    cursor: default;
  }
  .hdr {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-3);
    text-align: center;
    margin-bottom: 7px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, 38px);
    gap: 5px;
  }
  .nb {
    height: 36px;
    border-radius: 9px;
    border: 1px solid var(--line-2);
    background: var(--raised);
    color: var(--text);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.1s, transform 0.05s;
  }
  .nb:hover {
    background: var(--raised-2);
  }
  .nb.on,
  .nb:active {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: var(--accent);
    transform: scale(0.96);
  }
  .nb.sec {
    color: var(--text-2);
    background: var(--panel-2);
  }
  .fitb {
    margin-top: 6px;
    width: 100%;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--text-2);
    font-size: 12px;
    cursor: pointer;
  }
  .fitb:hover {
    background: var(--raised);
    color: var(--text);
  }
</style>
