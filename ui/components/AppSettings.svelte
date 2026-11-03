<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { THEMES, type ThemeId } from "../lib/theme";
  import { S, setLanguage, persist, buildSphere, IS_MOBILE, setTheme } from "../lib/store.svelte";

  function setQuality(q: "fast" | "normal" | "high") {
    S.settings.ui.quality = q;
    persist();
    buildSphere();
  }
</script>

<div class="prefs">
  <div class="group">
    <div class="label">{t("language")}</div>
    <div class="seg full">
      <button class:on={S.settings.lang === "auto"} onclick={() => setLanguage("auto")}>{t("langAuto")}</button>
      <button class:on={S.settings.lang === "pl"} onclick={() => setLanguage("pl")}>Polski</button>
      <button class:on={S.settings.lang === "en"} onclick={() => setLanguage("en")}>English</button>
      <button class:on={S.settings.lang === "de"} onclick={() => setLanguage("de")}>Deutsch</button>
    </div>
  </div>

  <div class="group">
    <div class="label">{t("theme")} · <span class="tname">{t(("theme_" + S.settings.ui.theme) as any)}</span></div>
    <div class="themes">
      {#each THEMES.filter((x) => !IS_MOBILE || !x.desktopOnly) as th}
        <button
          class="swatch"
          class:on={S.settings.ui.theme === th.id}
          data-tip={t(("theme_" + th.id) as any)}
          aria-label={t(("theme_" + th.id) as any)}
          style="background:{th.id === 'custom' ? S.settings.ui.customColor : th.swatch}"
          onclick={() => setTheme(th.id as ThemeId)}
        >{#if th.id === "custom"}<span class="rgb"></span>{/if}</button>
      {/each}
      {#if !IS_MOBILE}
        <label class="picker" data-tip={t("themePick")}>
          <input type="color" value={S.settings.ui.customColor} oninput={(e) => setTheme("custom", (e.target as HTMLInputElement).value)} />
          RGB
        </label>
      {/if}
    </div>
  </div>

  <div class="group">
    <div class="label">{t("previewQuality")}</div>
    <div class="seg full">
      <button class:on={S.settings.ui.quality === "fast"} onclick={() => setQuality("fast")}>{t("qFast")}</button>
      <button class:on={S.settings.ui.quality === "normal"} onclick={() => setQuality("normal")}>{t("qNormal")}</button>
      <button class:on={S.settings.ui.quality === "high"} onclick={() => setQuality("high")}>{t("qHigh")}</button>
    </div>
  </div>

  <label class="check">
    <input type="checkbox" bind:checked={S.settings.ui.checkUpdates} onchange={persist} />{t("updAuto")}
  </label>
</div>

<style>
  .prefs {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .label {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-3);
  }
  .tname {
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-2);
  }
  .themes {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .swatch {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 1px solid var(--line-2);
    cursor: pointer;
    position: relative;
    padding: 0;
  }
  .swatch.on {
    outline: 2px solid var(--text);
    outline-offset: 2px;
  }
  .rgb {
    position: absolute;
    right: -3px;
    bottom: -3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: conic-gradient(#f44, #fd4, #4f8, #4af, #a4f, #f44);
    border: 1px solid var(--surface);
  }
  .picker {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    font-size: 11px;
    color: var(--text-2);
    cursor: pointer;
  }
  .picker input {
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--line-2);
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .picker input::-webkit-color-swatch-wrapper {
    padding: 0;
  }
  .picker input::-webkit-color-swatch {
    border: 0;
    border-radius: 50%;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    color: var(--text-2);
    cursor: pointer;
    min-height: 32px;
  }
</style>
