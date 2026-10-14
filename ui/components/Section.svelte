<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  let { title, icon, open = $bindable(true), badge = "", tip = "", children }: { title: string; icon: string; open?: boolean; badge?: string; tip?: string; children: Snippet } = $props();
</script>

<section class:open>
  <button class="head" data-tip={tip || undefined} onclick={() => (open = !open)}>
    <span class="ic"><Icon name={icon} size={16} /></span>
    <span class="title">{title}</span>
    {#if badge}<span class="badge">{badge}</span>{/if}
    <span class="chev"><Icon name={open ? "up" : "down"} size={15} /></span>
  </button>
  {#if open}
    <div class="body">{@render children()}</div>
  {/if}
</section>

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    margin-bottom: 10px;
    overflow: hidden;
  }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 11px 12px;
    background: transparent;
    border: 0;
    cursor: pointer;
    text-align: left;
  }
  .head:hover {
    background: rgba(var(--fg-rgb), 0.025);
  }
  .ic {
    color: var(--text);
    display: flex;
  }
  .title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
    letter-spacing: 0.1px;
  }
  .badge {
    font-size: 11px;
    color: var(--text-2);
    border: 1px solid var(--line-2);
    padding: 1px 8px;
    border-radius: 20px;
  }
  .chev {
    color: var(--text-3);
    display: flex;
  }
  .body {
    padding: 2px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
</style>
