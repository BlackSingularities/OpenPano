<script lang="ts">
  // Global tooltip: any element with a `data-tip` attribute shows a styled hint on hover.
  import { onMount } from "svelte";

  let text = $state("");
  let x = $state(0);
  let y = $state(0);
  let above = $state(false);
  let visible = $state(false);
  let el: HTMLDivElement;
  let timer: any = null;
  let current: Element | null = null;

  function hide() {
    clearTimeout(timer);
    visible = false;
    current = null;
  }

  function show(target: Element) {
    const tip = target.getAttribute("data-tip");
    if (!tip) return;
    text = tip;
    const r = target.getBoundingClientRect();
    visible = true;
    requestAnimationFrame(() => {
      const w = el?.offsetWidth ?? 260;
      const h = el?.offsetHeight ?? 40;
      let nx = r.left + r.width / 2 - w / 2;
      nx = Math.max(8, Math.min(window.innerWidth - w - 8, nx));
      above = r.bottom + h + 12 > window.innerHeight;
      x = nx;
      y = above ? r.top - h - 8 : r.bottom + 8;
    });
  }

  onMount(() => {
    const over = (e: MouseEvent) => {
      const t = (e.target as Element)?.closest?.("[data-tip]");
      if (t === current) return;
      hide();
      if (!t) return;
      current = t;
      timer = setTimeout(() => current === t && show(t), 420);
    };
    document.addEventListener("mouseover", over);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("wheel", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      document.removeEventListener("mouseover", over);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("wheel", hide, true);
      window.removeEventListener("blur", hide);
    };
  });
</script>

<div class="tip" class:visible class:above bind:this={el} style="left:{x}px;top:{y}px">{text}</div>

<style>
  .tip {
    position: fixed;
    z-index: 200;
    max-width: 300px;
    padding: 8px 11px;
    border-radius: 9px;
    background: var(--surface);
    border: 1px solid rgba(var(--fg-rgb), 0.35);
    color: var(--text);
    font-size: 12px;
    line-height: 1.45;
    box-shadow: 0 10px 28px rgba(var(--bg-rgb), 0.5);
    pointer-events: none;
    opacity: 0;
    transform: translateY(-3px);
    transition: opacity 0.12s, transform 0.12s;
    white-space: pre-line;
  }
  .tip.above {
    transform: translateY(3px);
  }
  .tip.visible {
    opacity: 1;
    transform: none;
  }
</style>
