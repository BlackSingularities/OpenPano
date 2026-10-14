<script lang="ts">
  import type { Snippet } from "svelte";
  let { label, tip = "", children }: { label: string; tip?: string; children: Snippet } = $props();
</script>

<div class="field" data-tip={tip || undefined}>
  <span class="lbl" class:help={!!tip}>{label}</span>
  <div class="ctl">{@render children()}</div>
</div>

<style>
  .field {
    display: grid;
    grid-template-columns: 112px 1fr;
    align-items: center;
    gap: 10px;
  }
  .lbl.help {
    text-decoration: underline dotted rgba(var(--fg-rgb), 0.25);
    text-underline-offset: 3px;
    cursor: help;
  }
  .lbl {
    color: var(--text-2);
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .ctl :global(select) {
    width: 100%;
  }
</style>
