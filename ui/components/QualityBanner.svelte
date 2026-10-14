<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";

  let hidden = $state<unknown>(null);
  const al = $derived(app.align);
  const show = $derived((app.alignFailed || (al && al.qualityLevel !== "good")) && hidden !== (al ?? "failed"));
  const unaligned = $derived(al ? al.cameras.filter((c) => !c.aligned).length : 0);
  const reasons = $derived.by(() => {
    if (!al) return t("qFailed");
    const r: string[] = [];
    if (unaligned) r.push(`${unaligned} × ${t("qUnaligned")}`);
    if (al.issues.includes("highError")) r.push(`${t("qHighError")} (${al.rms.toFixed(1)} px)`);
    if (al.issues.includes("weakPairs")) r.push(t("qWeak"));
    return r.join(" · ");
  });
</script>

{#if show}
  <div class="banner" class:poor={app.alignFailed || al?.qualityLevel === "poor"}>
    <span class="ic"><Icon name="alert" size={18} /></span>
    <div class="txt">
      <b>{al ? `${t("qualityLabel")}: ${al.quality}/100 – ${al.qualityLevel === "poor" ? t("qPoor") : t("qFair")}` : t("alignFailed")}</b>
      <span>{reasons}. {t("qHint")}</span>
    </div>
    <button class="btn primary sm" onclick={() => (app.dialogs.cp = true)}><Icon name="target" size={14} />{t("cpOpen")}</button>
    <button class="btn ghost sm icon" data-tip={t("close")} onclick={() => (hidden = al ?? "failed")}><Icon name="x" size={14} /></button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 12px;
    margin: 8px 10px 0;
    border-radius: 14px;
    border: 1px solid var(--warn);
    background: var(--surface);
    text-align: left;
  }
  .banner.poor {
    border-color: var(--err);
  }
  .ic {
    color: var(--warn);
    display: flex;
  }
  .poor .ic {
    color: var(--err);
  }
  .txt {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .txt span {
    font-size: 12px;
    color: var(--text-2);
  }
</style>
