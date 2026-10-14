<script lang="ts">
  let {
    label,
    value = $bindable(),
    min,
    max,
    step = 1,
    unit = "",
    decimals = 0,
    disabled = false,
    tip = "",
    format,
    onchange,
    oncommit,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    unit?: string;
    decimals?: number;
    disabled?: boolean;
    tip?: string;
    format?: (v: number) => string;
    onchange?: (v: number) => void;
    oncommit?: (v: number) => void;
  } = $props();
  const pct = $derived(((value - min) / (max - min)) * 100);
  function set(v: number) {
    if (!isFinite(v)) return;
    value = Math.min(max, Math.max(min, v));
    onchange?.(value);
  }
</script>

<div class="row" data-tip={tip || undefined}>
  <span class="lbl" class:help={!!tip}>{label}</span>
  <input
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    value={value}
    style="--p:{pct}%"
    oninput={(e) => set(+(e.target as HTMLInputElement).value)}
    onchange={() => oncommit?.(value)}
  />
  {#if format}
    <span class="val">{format(value)}</span>
  {:else}
    <span class="num">
      <input
        type="number"
        {min}
        {max}
        {step}
        {disabled}
        value={+value.toFixed(decimals)}
        onchange={(e) => {
          set(+(e.target as HTMLInputElement).value);
          oncommit?.(value);
        }}
      />{#if unit}<span class="unit">{unit}</span>{/if}
    </span>
  {/if}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 112px 1fr auto;
    align-items: center;
    gap: 10px;
  }
  .lbl.help {
    text-decoration: underline dotted rgba(255, 255, 255, 0.25);
    text-underline-offset: 3px;
    cursor: help;
  }
  .lbl {
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  input[type="range"] {
    width: 100%;
    min-width: 60px;
  }
  .num {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .num input {
    width: 64px;
  }
  .unit,
  .val {
    color: var(--text-3);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .val {
    min-width: 46px;
    text-align: right;
    color: var(--text-2);
  }
</style>
