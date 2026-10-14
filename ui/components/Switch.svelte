<script lang="ts">
  let { label, checked = $bindable(), disabled = false, tip = "", onchange }: { label: string; checked: boolean; disabled?: boolean; tip?: string; onchange?: (v: boolean) => void } = $props();
</script>

<button
  class="sw"
  class:on={checked}
  {disabled}
  data-tip={tip || undefined}
  role="switch"
  aria-checked={checked}
  onclick={() => {
    checked = !checked;
    onchange?.(checked);
  }}
>
  <span class="lbl">{label}</span>
  <span class="track"><span class="knob"></span></span>
</button>

<style>
  .sw {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 3px 0;
    background: none;
    border: 0;
    cursor: pointer;
    text-align: left;
    color: var(--text);
  }
  .sw:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .lbl {
    color: var(--text-2);
  }
  .sw.on .lbl {
    color: var(--text);
  }
  .track {
    width: 34px;
    height: 19px;
    border-radius: 20px;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px rgba(var(--fg-rgb), 0.35);
    position: relative;
    transition: background 0.15s;
    flex: none;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    background: rgba(var(--fg-rgb), 0.75);
    transition: transform 0.15s, background 0.15s;
    box-shadow: 0 1px 3px rgba(var(--bg-rgb), 0.4);
  }
  .on .track {
    background: var(--sel-bg);
    box-shadow: none;
  }
  .on .knob {
    background: var(--surface);
  }
  .on .knob {
    transform: translateX(15px);
  }
</style>
