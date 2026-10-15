<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { S, persist, IS_MOBILE } from "../lib/store.svelte";
  import { upd, openUpdate } from "../lib/updates.svelte";

  let { hidden = $bindable(false) }: { hidden?: boolean } = $props();

  const IS_IOS = /iphone|ipad|ipod/i.test(navigator.userAgent);
  const show = $derived(!!upd.available && !hidden && S.settings.ui.skippedVersion !== upd.available.version);

  function skip() {
    if (upd.available) S.settings.ui.skippedVersion = upd.available.version;
    persist();
  }
</script>

{#if show && upd.available}
  <div class="upd" class:mobile={IS_MOBILE} role="status">
    <span class="ic"><Icon name="download" size={18} /></span>
    <div class="txt">
      <b>{t("updAvailable")}: {upd.available.version}</b>
      <span>{t("updText")} {upd.current}.{#if IS_IOS} {t("updIos")}{/if}</span>
      <div class="btns">
        <button class="btn primary sm" onclick={() => openUpdate(true)}><Icon name="download" size={14} />{t("updDownload")}</button>
        {#if upd.available.download}<button class="btn sm" onclick={() => openUpdate(false)}>{t("updNotes")}</button>{/if}
        <button class="btn ghost sm" onclick={skip}>{t("updSkip")}</button>
      </div>
    </div>
    <button class="btn ghost sm icon" data-tip={t("updLater")} onclick={() => (hidden = true)}><Icon name="x" size={14} /></button>
  </div>
{/if}

<style>
  .upd {
    position: fixed;
    top: calc(64px + env(safe-area-inset-top));
    right: 14px;
    z-index: 95;
    width: 380px;
    max-width: calc(100vw - 28px);
    display: flex;
    align-items: flex-start;
    gap: 11px;
    padding: 12px 10px 12px 13px;
    border-radius: 14px;
    border: 1px solid var(--line-2);
    background: var(--surface);
    box-shadow: var(--shadow);
    animation: pop 0.18s ease-out;
  }
  .upd.mobile {
    top: auto;
    right: 10px;
    left: 10px;
    width: auto;
    max-width: none;
    bottom: calc(76px + env(safe-area-inset-bottom));
  }
  .ic {
    width: 32px;
    height: 32px;
    flex: none;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--sel-bg);
    color: var(--sel-fg);
  }
  .txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .txt span {
    font-size: 12px;
    color: var(--text-2);
    line-height: 1.4;
  }
  .btns {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 7px;
  }
  @keyframes pop {
    from {
      transform: translateY(-6px);
      opacity: 0;
    }
  }
</style>
