<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../lib/app.svelte";
  import {
    commands,
    events,
    type GdriveCacheStats,
    type GdriveFolderItem,
    type GdriveStatus,
    type GdriveSyncedFolder,
    type GdriveSyncPhase,
  } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  const CACHE_PRESETS_MB = [512, 1024, 2048, 4096, 8192] as const;

  let status = $state<GdriveStatus | null>(null);
  let clientIdDraft = $state("");
  let clientIdDirty = $state(false);
  let clientSecretDraft = $state("");
  let savingConfig = $state(false);
  let signingIn = $state(false);

  let syncedFolders = $state<GdriveSyncedFolder[]>([]);
  let browserOpen = $state(false);
  let browserLoading = $state(false);
  let browserStack = $state<{ id: string | null; name: string }[]>([{ id: null, name: "" }]);
  let browserEntries = $state<GdriveFolderItem[]>([]);

  let syncPhase = $state<GdriveSyncPhase | "idle">("idle");
  let syncFolder = $state("");
  let syncDone = $state(0);
  let syncTotal = $state(0);
  let syncSummary = $state<string | null>(null);

  let cacheStats = $state<GdriveCacheStats | null>(null);
  let clearingCache = $state(false);

  async function refreshAll() {
    const [st, folders, stats] = await Promise.all([
      app.run(commands.gdriveStatus()),
      app.run(commands.gdriveSyncedFolders()),
      app.run(commands.gdriveCacheStats()),
    ]);
    if (st) {
      status = st;
      if (!clientIdDirty) clientIdDraft = st.clientId;
      if (st.syncing && syncPhase === "idle") syncPhase = "listing";
    }
    if (folders) syncedFolders = folders;
    if (stats) cacheStats = stats;
  }

  void refreshAll();

  const unlistenProgress = events.gdriveSyncProgressEvent.listen((e) => {
    syncPhase = e.payload.phase;
    syncFolder = e.payload.folderName;
    syncDone = e.payload.done;
    syncTotal = e.payload.total;
  });

  const unlistenFinished = events.gdriveSyncFinishedEvent.listen((e) => {
    syncPhase = "idle";
    const p = e.payload;
    if (p.error) {
      syncSummary = p.error;
    } else if (p.added + p.updated + p.removed > 0) {
      syncSummary = t("settings.gdrive.syncDone", {
        added: p.added,
        updated: p.updated,
        removed: p.removed,
      });
    } else {
      syncSummary = t("settings.gdrive.syncUpToDate");
    }
    void refreshAll();
  });

  onDestroy(() => {
    unlistenProgress.then((f) => f());
    unlistenFinished.then((f) => f());
  });

  async function saveConfig() {
    savingConfig = true;
    const secretArg = clientSecretDraft.trim() ? clientSecretDraft.trim() : null;
    const next = await app.run(commands.gdriveSaveConfig(clientIdDraft.trim(), secretArg));
    savingConfig = false;
    if (next) {
      status = next;
      clientIdDraft = next.clientId;
      clientIdDirty = false;
      clientSecretDraft = "";
      app.notify("info", t("settings.gdrive.configSaved"));
    }
  }

  async function clearSecret() {
    savingConfig = true;
    const next = await app.run(commands.gdriveSaveConfig(clientIdDraft.trim(), ""));
    savingConfig = false;
    if (next) {
      status = next;
      clientIdDraft = next.clientId;
      clientIdDirty = false;
      clientSecretDraft = "";
      app.notify("info", t("settings.gdrive.configSaved"));
    }
  }

  async function signIn() {
    signingIn = true;
    const next = await app.run(commands.gdriveSignIn());
    signingIn = false;
    if (next) {
      status = next;
      void refreshAll();
    }
  }

  async function cancelSignIn() {
    await app.run(commands.gdriveCancelSignIn());
    signingIn = false;
  }

  async function signOut() {
    const next = await app.run(commands.gdriveSignOut());
    if (next) {
      status = next;
      browserOpen = false;
      browserEntries = [];
    }
  }

  async function openBrowser() {
    browserOpen = !browserOpen;
    if (browserOpen) {
      browserStack = [{ id: null, name: t("settings.gdrive.myDrive") }];
      await loadBrowserFolder(null);
    }
  }

  async function loadBrowserFolder(parentId: string | null) {
    browserLoading = true;
    const list = await app.run(commands.gdriveListFolders(parentId));
    browserLoading = false;
    browserEntries = list ?? [];
  }

  async function enterFolder(folder: GdriveFolderItem) {
    browserStack = [...browserStack, { id: folder.id, name: folder.name }];
    await loadBrowserFolder(folder.id);
  }

  async function goToCrumb(index: number) {
    browserStack = browserStack.slice(0, index + 1);
    const current = browserStack[browserStack.length - 1];
    await loadBrowserFolder(current?.id ?? null);
  }

  async function addFolder(folder: GdriveFolderItem) {
    const added = await app.run(commands.gdriveAddFolder(folder.id, folder.name));
    if (added) {
      syncedFolders = (await app.run(commands.gdriveSyncedFolders())) ?? syncedFolders;
    }
  }

  async function removeFolder(folderId: string) {
    await app.run(commands.gdriveRemoveFolder(folderId));
    syncedFolders = (await app.run(commands.gdriveSyncedFolders())) ?? syncedFolders;
  }

  async function syncNow() {
    syncSummary = null;
    await app.run(commands.gdriveSyncNow());
  }

  async function setCacheLimit(mb: number) {
    const next = await app.run(commands.gdriveSetCacheLimit(mb));
    if (next) cacheStats = next;
  }

  async function clearCache() {
    clearingCache = true;
    const next = await app.run(commands.gdriveClearCache());
    clearingCache = false;
    if (next) {
      cacheStats = next;
      app.notify("info", t("settings.gdrive.cacheCleared"));
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
    return `${bytes} B`;
  }

  function formatLimitPreset(mb: number): string {
    return mb >= 1024 ? `${mb / 1024} GB` : `${mb} MB`;
  }

  let syncing = $derived(syncPhase !== "idle" || Boolean(status?.syncing));
  let syncedIds = $derived(new Set(syncedFolders.map((f) => f.folderId)));
  let cacheLimitMb = $derived(
    cacheStats ? Math.round(cacheStats.maxBytes / (1024 * 1024)) : 2048,
  );
  let syncText = $derived.by(() => {
    if (syncPhase === "listing") {
      return t("settings.gdrive.syncListing", { folder: syncFolder || "Google Drive" });
    }
    if (syncPhase === "reading") {
      return t("settings.gdrive.syncReading", {
        folder: syncFolder || "Google Drive",
        done: syncDone,
        total: syncTotal,
      });
    }
    if (syncPhase === "writing") return t("settings.gdrive.syncWriting");
    return syncSummary;
  });
</script>

<section aria-labelledby="gdrive-title">
  <div class="group-head">
    <h3 id="gdrive-title">{t("settings.gdrive.title")}</h3>
  </div>
  <div class="card soft">
    <div class="row">
      <span class="icon"><Icon name="cloud" size={18} /></span>
      <span class="info">
        <span class="name">{t("settings.gdrive.account")}</span>
        <span class="dim small">
          {#if status?.authenticated}
            {status.accountEmail
              ? t("settings.gdrive.connectedAs", { email: status.accountEmail })
              : t("settings.gdrive.connected")}
          {:else}
            {t("settings.gdrive.subtitle")}
          {/if}
        </span>
      </span>
      {#if status?.authenticated}
        <button class="btn small" onclick={signOut}>{t("settings.gdrive.signOut")}</button>
      {:else if signingIn}
        <div class="btn-group">
          <span class="dim small">{t("settings.gdrive.signingIn")}</span>
          <button class="btn small" onclick={cancelSignIn}>{t("settings.gdrive.cancelSignIn")}</button>
        </div>
      {:else}
        <button class="btn primary small" disabled={!status?.configured} onclick={signIn}>
          {t("settings.gdrive.signIn")}
        </button>
      {/if}
    </div>

    <div class="cred-block">
      <div class="cred-fields">
        <label class="field">
          <span class="field-label">{t("settings.gdrive.clientId")}</span>
          <input
            type="text"
            class="text-input"
            bind:value={clientIdDraft}
            oninput={() => (clientIdDirty = true)}
            placeholder={t("settings.gdrive.clientIdPlaceholder")}
            autocomplete="off"
            spellcheck="false"
          />
        </label>
        <label class="field">
          <span class="field-label">{t("settings.gdrive.clientSecret")}</span>
          <input
            type="password"
            class="text-input"
            bind:value={clientSecretDraft}
            placeholder={status?.hasClientSecret
              ? t("settings.gdrive.clientSecretSaved")
              : t("settings.gdrive.clientSecretPlaceholder")}
            autocomplete="off"
          />
        </label>
      </div>
      <div class="cred-foot">
        <span class="dim small">
          {status?.envOverride ? t("settings.gdrive.envHint") : t("settings.gdrive.oauthHint")}
        </span>
        <div class="btn-group">
          {#if status?.hasClientSecret}
            <button class="btn small" disabled={savingConfig} onclick={clearSecret}>
              {t("settings.gdrive.clearSecret")}
            </button>
          {/if}
          <button class="btn small" disabled={savingConfig} onclick={saveConfig}>
            {t("settings.gdrive.saveConfig")}
          </button>
        </div>
      </div>
    </div>

    {#if status?.authenticated}
      {#each syncedFolders as folder (folder.folderId)}
        <div class="row">
          <span class="icon"><Icon name="folder" size={18} /></span>
          <span class="info">
            <span class="name">{folder.name}</span>
            <span class="dim small">{t("tracks.gdriveBadge")}</span>
          </span>
          <button class="btn small" onclick={() => removeFolder(folder.folderId)}>
            {t("common.remove")}
          </button>
        </div>
      {:else}
        <p class="dim pad">{t("settings.gdrive.foldersEmpty")}</p>
      {/each}

      <div class="row actions">
        <button class="btn primary small" onclick={openBrowser}>
          <Icon name={browserOpen ? "close" : "add"} size={16} />
          {browserOpen ? t("settings.gdrive.closeBrowser") : t("settings.gdrive.addFolder")}
        </button>
        <span class="grow"></span>
        <button
          class="btn small"
          disabled={syncing || !syncedFolders.length}
          onclick={syncNow}
        >
          <Icon name="refresh" size={16} /> {t("settings.gdrive.syncNow")}
        </button>
      </div>

      {#if browserOpen}
        <div class="browser">
          <div class="crumbs">
            {#each browserStack as crumb, i (crumb.id ?? "root")}
              {#if i > 0}<span class="sep">/</span>{/if}
              <button
                class="crumb"
                class:current={i === browserStack.length - 1}
                onclick={() => goToCrumb(i)}
              >
                {crumb.name || t("settings.gdrive.myDrive")}
              </button>
            {/each}
          </div>
          {#if browserLoading}
            <p class="dim small pad-sm">{t("settings.gdrive.loadingFolders")}</p>
          {:else if !browserEntries.length}
            <p class="dim small pad-sm">{t("settings.gdrive.noSubfolders")}</p>
          {:else}
            <div class="folder-list">
              {#each browserEntries as entry (entry.id)}
                <div class="folder-item">
                  <button class="folder-open" onclick={() => enterFolder(entry)}>
                    <Icon name="folder" size={16} />
                    <span class="folder-name">{entry.name}</span>
                  </button>
                  <button
                    class="btn small"
                    disabled={syncedIds.has(entry.id)}
                    onclick={() => addFolder(entry)}
                  >
                    {syncedIds.has(entry.id) ? "✓" : t("settings.gdrive.selectFolder")}
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if syncText}
        <div class="scan" aria-live="polite">
          {#if syncing}
            <div class="progress">
              <div
                class="fill"
                class:indeterminate={syncPhase !== "reading" || !syncTotal}
                style:width={syncTotal ? `${(100 * syncDone) / syncTotal}%` : "30%"}
              ></div>
            </div>
          {/if}
          <span class="dim small">{syncText}</span>
        </div>
      {/if}
    {/if}

    <div class="row">
      <span class="icon"><Icon name="disc" size={18} /></span>
      <span class="info">
        <span class="name">{t("settings.gdrive.cache")}</span>
        <span class="dim small">
          {#if cacheStats}
            {t("settings.gdrive.cacheUsage", {
              used: formatBytes(cacheStats.totalBytes),
              limit: formatBytes(cacheStats.maxBytes),
              files: cacheStats.fileCount,
            })}
          {:else}
            —
          {/if}
        </span>
      </span>
      <div class="cache-controls">
        <div class="segmented" role="radiogroup" aria-label={t("settings.gdrive.cacheLimit")}>
          {#each CACHE_PRESETS_MB as mb (mb)}
            <button
              class="chip"
              role="radio"
              aria-checked={cacheLimitMb === mb}
              onclick={() => setCacheLimit(mb)}
            >
              {formatLimitPreset(mb)}
            </button>
          {/each}
        </div>
        <button
          class="btn small"
          disabled={clearingCache || !cacheStats?.totalBytes}
          onclick={clearCache}
        >
          {t("settings.gdrive.clearCache")}
        </button>
      </div>
    </div>
  </div>
</section>

<style>
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 0 4px 10px;
  }
  h3 {
    margin: 0;
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .card {
    border-radius: var(--r-md);
    overflow: hidden;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 18px;
    border-top: 1px solid var(--hairline);
  }
  .row:first-child {
    border-top: none;
  }
  .row.actions {
    flex-wrap: wrap;
    gap: 10px;
  }
  .grow {
    flex: 1;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: var(--r-sm);
    background: var(--soft);
    color: var(--ink-2);
    flex-shrink: 0;
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: var(--fs-body);
    font-weight: 600;
    color: var(--ink);
  }
  .dim {
    color: var(--ink-2);
  }
  .small {
    font-size: var(--fs-small);
  }
  .pad {
    margin: 0;
    padding: 14px 18px;
    border-top: 1px solid var(--hairline);
    font-size: var(--fs-small);
  }
  .pad-sm {
    margin: 0;
    padding: 10px 12px;
  }
  .btn-group {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .cred-block {
    padding: 14px 18px;
    border-top: 1px solid var(--hairline);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .cred-fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field-label {
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--ink-2);
  }
  .text-input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 12px;
    border-radius: var(--r-sm);
    border: 1px solid var(--hairline);
    background: rgb(0 0 0 / 0.32);
    color: var(--ink);
    font-size: var(--fs-small);
    font-family: var(--font);
  }
  .text-input:focus {
    outline: none;
    border-color: var(--ink-2);
  }
  .cred-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .browser {
    border-top: 1px solid var(--hairline);
    background: rgb(0 0 0 / 0.22);
    padding: 10px 14px 14px;
  }
  .crumbs {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 8px;
  }
  .crumb {
    padding: 4px 8px;
    border-radius: var(--r-xs);
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .crumb:hover {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .crumb.current {
    color: var(--ink);
    font-weight: 600;
  }
  .sep {
    color: var(--ink-3);
    font-size: var(--fs-small);
  }
  .folder-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 220px;
    overflow-y: auto;
  }
  .folder-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: var(--r-sm);
  }
  .folder-item:hover {
    background: var(--soft);
  }
  .folder-open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    color: var(--ink);
    text-align: left;
  }
  .folder-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-small);
  }
  .scan {
    padding: 10px 18px 14px;
    border-top: 1px solid var(--hairline);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .progress {
    height: 4px;
    border-radius: 2px;
    background: var(--soft);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--ink);
    transition: width 180ms ease;
  }
  .fill.indeterminate {
    width: 30%;
  }
  .cache-controls {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .segmented {
    display: inline-flex;
    padding: 3px;
    border-radius: var(--r-pill);
    background: rgb(0 0 0 / 0.35);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .chip {
    padding: 5px 10px;
    border-radius: var(--r-pill);
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--ink-2);
  }
  .chip[aria-checked="true"] {
    background: var(--ink);
    color: #000;
  }
</style>
