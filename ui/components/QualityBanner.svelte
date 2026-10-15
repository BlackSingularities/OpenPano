<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { app, nameList, imageName } from "../lib/store.svelte";

  let hidden = $state<unknown>(null);
  const al = $derived(app.align);
  const rep = $derived(app.report);
  // a new attempt (new report object) brings a dismissed banner back
  const key = $derived(app.report ?? al ?? "failed");
  const unaligned = $derived(rep?.unaligned ?? (al ? al.cameras.filter((c) => !c.aligned).map((c) => c.id) : []));
  const badPairs = $derived(rep ? rep.pairs.filter((p) => p.status === "highError").length : 0);
  const show = $derived((app.alignFailed || (al && (al.qualityLevel !== "good" || badPairs > 0))) && hidden !== key);
  const reasons = $derived.by(() => {
    if (!al) {
      const best = rep?.pairs.slice().sort((x, y) => y.inliers - x.inliers || y.matches - x.matches)[0];
      const r = [t("qNoPairs")];
      if (best) r.push(`${t("qBestPair")}: ${imageName(best.a)} ↔ ${imageName(best.b)} (${best.matches} ${t("qMatchesOf")} ${best.inliers})`);
      return r.join(" · ");
    }
    const r: string[] = [];
    if (unaligned.length) r.push(`${t("qNotConnected")}: ${nameList(unaligned)}`);
    if (badPairs) r.push(`${badPairs} × ${t("qBadPairs")}`);
    if (al.issues.includes("highError")) r.push(`${t("qHighError")} (${al.rms.toFixed(1)} px)`);
    if (al.issues.includes("weakPairs") && !badPairs) r.push(t("qWeak"));
    return r.join(" · ");
  });
</script>

{#if show}
  <div class="banner" class:poor={app.alignFailed || al?.qualityLevel === "poor"}>
    <span class="ic"><Icon name="alert" size={18} /></span>
    <div class="txt">
      <b>{al ? `${t("qualityLabel")}: ${al.quality}/100 – ${al.qualityLevel === "poor" ? t("qPoor") : al.qualityLevel === "fair" ? t("qFair") : t("qGood")}` : t("alignFailed")}</b>
      <span>{reasons}. {t("qHint")}</span>
    </div>
    <button class="btn primary sm" onclick={() => (app.dialogs.cp = true)}><Icon name="target" size={14} />{t("cpOpen")}</button>
    <button class="btn ghost sm icon" data-tip={t("close")} onclick={() => (hidden = key)}><Icon name="x" size={14} /></button>
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
