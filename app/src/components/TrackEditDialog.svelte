<script lang="ts">
  // The metadata editor: the tags stored in the track's own file, read fresh when it opens and
  // written back into that same file on Save, so a rescan or another player sees the edit.
  // Cover art and lyrics are part of it, and online catalogues and lyrics services can fill
  // the fields in on request; nothing is written before Save.
  import { app } from "../lib/app.svelte";
  import {
    commands,
    type CoverImage,
    type MetadataCandidate,
    type TrackEdit,
  } from "../lib/bindings";
  import { artUrl } from "../lib/art";
  import { duration } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { forgetLyrics } from "../lib/lyrics";
  import {
    MAX_COVER_BYTES,
    applyCandidate,
    coverDataUrl,
    coverFromDataUrl,
    formFromTags,
    numberField,
    splitNames,
    type TagForm,
  } from "../lib/tagEdit";

  let el: HTMLDialogElement | undefined = $state();
  let picker: HTMLInputElement | undefined = $state();
  /** The track whose tags are on screen; a different one reloads the fields. */
  let shown: number | null = null;
  /** Whether the file's tags have loaded. */
  let ready = $state(false);
  let saving = $state(false);
  /** The path of the track being edited, kept while the dialog is open. */
  let path = $state("");
  /** The track's cover as the library shows it, which may come from its folder. */
  let libraryArt = $state<string | null>(null);

  let form = $state<TagForm>(emptyForm());
  /** The cover embedded in the file when it opened. */
  let fileCover = $state<CoverImage | null>(null);
  /** What Save does to the embedded cover; a new image is kept whole so it can be shown. */
  let cover = $state<{ kind: "keep" } | { kind: "remove" } | { kind: "set"; image: CoverImage }>({
    kind: "keep",
  });
  /** The lyrics in the file when it opened, to save only a change. */
  let fileLyrics = "";
  let lyrics = $state("");

  let lookingUp = $state(false);
  /** Candidates from the last lookup; `null` when none was asked for. */
  let candidates = $state<MetadataCandidate[] | null>(null);
  /** Small covers for the candidates, by URL, as they arrive. */
  let thumbs = $state<Record<string, string>>({});
  let applying = $state(false);
  let findingLyrics = $state(false);
  /** Bumped on every reload and lookup, so the answer to an older lookup is dropped. */
  let lookups = 0;

  function emptyForm(): TagForm {
    return {
      title: "",
      artists: "",
      album: "",
      albumArtists: "",
      trackNo: "",
      discNo: "",
      year: "",
      genre: "",
    };
  }

  $effect(() => {
    if (!el) return;
    const track = app.editing;
    if (!track) {
      shown = null;
      ready = false;
      if (el.open) el.close();
      return;
    }
    if (!el.open) el.showModal();
    if (shown === track.id) return;
    shown = track.id;
    lookups++;
    ready = false;
    saving = false;
    path = track.path;
    libraryArt = artUrl(track.art, 300);
    form = emptyForm();
    fileCover = null;
    cover = { kind: "keep" };
    fileLyrics = "";
    lyrics = "";
    candidates = null;
    thumbs = {};
    lookingUp = false;
    applying = false;
    findingLyrics = false;
    void load(track.id);
  });

  async function load(id: number) {
    const found = await app.run(commands.trackTags(id));
    if (shown !== id || !found) return;
    form = formFromTags(found.tags);
    fileCover = found.cover;
    fileLyrics = found.tags.lyrics ?? "";
    lyrics = fileLyrics;
    ready = true;
  }

  /** What the cover box shows, and what it means. */
  let coverView = $derived.by((): { src: string | null; note: string } => {
    if (cover.kind === "set") {
      return { src: coverDataUrl(cover.image), note: t("tags.coverNew") };
    }
    if (cover.kind === "remove") return { src: null, note: t("tags.coverRemoved") };
    if (fileCover) return { src: coverDataUrl(fileCover), note: "" };
    if (libraryArt) return { src: libraryArt, note: t("tags.coverFolder") };
    return { src: null, note: t("tags.coverNone") };
  });

  let busy = $derived(saving || applying);
  let canSave = $derived(ready && form.title.trim().length > 0 && !busy);
  let query = $derived({ title: form.title, artist: form.artists, album: form.album });

  function pickCover() {
    picker?.click();
  }

  function onPicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > MAX_COVER_BYTES) {
      app.notify("error", t("tags.coverTooLarge"));
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const image = typeof reader.result === "string" ? coverFromDataUrl(reader.result) : null;
      if (!image) {
        app.notify("error", t("tags.coverInvalid"));
        return;
      }
      cover = { kind: "set", image };
    };
    reader.onerror = () => app.notify("error", t("tags.coverInvalid"));
    reader.readAsDataURL(file);
  }

  async function lookup() {
    const track = app.editing;
    if (!track || lookingUp) return;
    const mine = ++lookups;
    lookingUp = true;
    candidates = null;
    thumbs = {};
    const found = await app.run(commands.trackLookup(track.id, query));
    if (mine !== lookups) return;
    lookingUp = false;
    if (!found) return;
    candidates = found;
    for (const url of new Set(found.map((c) => c.thumbUrl).filter((u): u is string => !!u))) {
      // A missing thumbnail is not worth a notice; the row shows without it.
      commands.coverDownload(url).then(
        (image) => {
          if (mine === lookups && image) thumbs[url] = coverDataUrl(image);
        },
        () => {},
      );
    }
  }

  async function apply(found: MetadataCandidate) {
    const id = shown;
    form = applyCandidate(form, found);
    candidates = null;
    if (found.coverUrl) {
      applying = true;
      const image = await app.run(commands.coverDownload(found.coverUrl));
      if (id !== shown) return;
      applying = false;
      if (image) cover = { kind: "set", image };
    }
    app.notify("info", t("tags.lookupApplied", { source: found.source }));
  }

  async function findLyrics() {
    const track = app.editing;
    if (!track || findingLyrics) return;
    const id = track.id;
    findingLyrics = true;
    const found = await app.run(commands.trackFindLyrics(id, query));
    if (id !== shown) return;
    findingLyrics = false;
    if (found === undefined) return;
    if (!found) app.notify("info", t("tags.lyricsNone"));
    else if (found.instrumental) app.notify("info", t("tags.lyricsInstrumental", { source: found.source }));
    else {
      lyrics = found.text;
      app.notify("info", t("tags.lyricsFound", { source: found.source }));
    }
  }

  function candidateLine(c: MetadataCandidate): string {
    return [c.album, c.year?.toString(), c.durationMs ? duration(c.durationMs) : null]
      .filter(Boolean)
      .join(" · ");
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const track = app.editing;
    if (!track || !canSave) return;
    saving = true;
    const lyricsChanged = lyrics !== fileLyrics;
    const edit: TrackEdit = {
      title: form.title.trim(),
      artists: splitNames(form.artists),
      album: form.album.trim(),
      albumArtists: splitNames(form.albumArtists),
      trackNo: numberField(form.trackNo),
      discNo: numberField(form.discNo),
      year: numberField(form.year),
      genre: form.genre.trim(),
      lyrics: lyricsChanged ? lyrics : null,
      cover: cover.kind === "set" ? { kind: "set", data: cover.image.data } : cover,
    };
    const saved = await app.run(commands.trackUpdateTags(track.id, edit));
    saving = false;
    // A failure is already a notice with the reason; the editor stays open with the values.
    if (saved === undefined) return;
    if (lyricsChanged) {
      forgetLyrics(track.id);
      app.lyricsVersion++;
    }
    app.closeEditor();
  }
</script>

<!-- Escape fires `cancel`, then `close`; both close the editor. -->
<dialog bind:this={el} class="glass" aria-labelledby="tags-title" onclose={() => app.closeEditor()}>
  {#if app.editing}
    <form onsubmit={submit}>
      <header>
        <div class="heading">
          <h2 id="tags-title">{t("tags.editTitle")}</h2>
          <p class="path dim" title={path}>{path}</p>
        </div>
        {#if ready}
          <button
            type="button"
            class="btn small"
            disabled={lookingUp || busy || !form.title.trim()}
            title={t("tags.lookupHint")}
            onclick={lookup}
          >
            {lookingUp ? t("tags.lookupBusy") : t("tags.lookup")}
          </button>
        {/if}
      </header>
      {#if ready}
        {#if lookingUp || candidates}
          <section class="results soft" aria-label={t("tags.lookupResults")} aria-busy={lookingUp}>
            <div class="results-head">
              <span class="dim">{t("tags.lookupHint")}</span>
              <button type="button" class="btn small" onclick={() => (candidates = null)} disabled={lookingUp}>
                {t("tags.lookupClose")}
              </button>
            </div>
            {#if lookingUp}
              <p class="dim">{t("tags.lookupBusy")}</p>
            {:else if candidates && candidates.length === 0}
              <p class="dim">{t("tags.lookupNone")}</p>
            {:else if candidates}
              <ul>
                {#each candidates as c, i (i)}
                  <li>
                    <button
                      type="button"
                      class="candidate"
                      aria-label={t("tags.lookupUse", { title: c.title, source: c.source })}
                      onclick={() => apply(c)}
                    >
                      {#if c.thumbUrl && thumbs[c.thumbUrl]}
                        <img src={thumbs[c.thumbUrl]} alt="" />
                      {:else}
                        <span class="thumb-blank" aria-hidden="true"></span>
                      {/if}
                      <span class="candidate-text">
                        <strong>{c.title}</strong>
                        <span>{c.artists.join(", ")}</span>
                        <span class="dim">{candidateLine(c)}</span>
                      </span>
                      <span class="badges">
                        <span class="badge">{c.source}</span>
                        {#if c.lengthMatches}<span class="badge ok">{t("tags.lookupMatch")}</span>{/if}
                      </span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {/if}
        <div class="top">
          <div class="cover">
            <div class="cover-box" class:faded={cover.kind === "keep" && !fileCover && libraryArt}>
              {#if coverView.src}
                <img src={coverView.src} alt={t("tags.cover")} />
              {:else}
                <span class="dim">{t("tags.cover")}</span>
              {/if}
            </div>
            {#if coverView.note}<p class="dim note">{coverView.note}</p>{/if}
            <div class="cover-actions">
              <button type="button" class="btn small" disabled={busy} onclick={pickCover}>
                {t("tags.coverPick")}
              </button>
              {#if cover.kind !== "keep"}
                <button type="button" class="btn small" disabled={busy} onclick={() => (cover = { kind: "keep" })}>
                  {t("tags.coverUndo")}
                </button>
              {:else if fileCover}
                <button type="button" class="btn small" disabled={busy} onclick={() => (cover = { kind: "remove" })}>
                  {t("tags.coverRemove")}
                </button>
              {/if}
            </div>
            <input
              bind:this={picker}
              type="file"
              accept="image/jpeg,image/png"
              hidden
              onchange={onPicked}
            />
          </div>
          <div class="fields">
            <label class="wide">
              {t("tags.title")}
              <!-- svelte-ignore a11y_autofocus -->
              <input bind:value={form.title} required maxlength="500" autofocus />
            </label>
            <label class="wide">
              {t("tags.artists")}
              <input bind:value={form.artists} placeholder={t("tags.namesHint")} />
            </label>
            <label>
              {t("tags.album")}
              <input bind:value={form.album} />
            </label>
            <label>
              {t("tags.albumArtists")}
              <input bind:value={form.albumArtists} placeholder={t("tags.namesHint")} />
            </label>
          </div>
        </div>
        <div class="fields">
          <label>
            {t("tags.trackNo")}
            <input type="number" min="0" max="9999" bind:value={form.trackNo} />
          </label>
          <label>
            {t("tags.discNo")}
            <input type="number" min="0" max="9999" bind:value={form.discNo} />
          </label>
          <label>
            {t("tags.year")}
            <input type="number" min="0" max="9999" bind:value={form.year} />
          </label>
          <label>
            {t("tags.genre")}
            <input bind:value={form.genre} />
          </label>
        </div>
        <div class="lyrics">
          <div class="lyrics-head">
            <label for="tags-lyrics">{t("tags.lyrics")}</label>
            <button
              type="button"
              class="btn small"
              disabled={findingLyrics || busy || !form.title.trim()}
              title={t("tags.lyricsFindHint")}
              onclick={findLyrics}
            >
              {findingLyrics ? t("tags.lyricsFinding") : t("tags.lyricsFind")}
            </button>
          </div>
          <textarea
            id="tags-lyrics"
            bind:value={lyrics}
            rows="7"
            spellcheck="false"
            placeholder={t("tags.lyricsHint")}
          ></textarea>
        </div>
        <p class="dim hint">{t("tags.inFile")}</p>
        <div class="actions">
          <button type="button" class="btn" disabled={saving} onclick={() => app.closeEditor()}>
            {t("common.cancel")}
          </button>
          <button type="submit" class="btn primary" disabled={!canSave}>
            {saving ? t("tags.saving") : t("tags.save")}
          </button>
        </div>
      {:else}
        <p class="dim">{t("tags.reading")}</p>
      {/if}
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(720px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    overflow-y: auto;
    padding: 24px;
    border: none;
    border-radius: var(--r-lg);
    color: var(--ink);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.5);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h2 {
    font-size: var(--fs-title);
  }
  .path {
    margin: 0;
    font-size: var(--fs-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .top {
    display: grid;
    grid-template-columns: 168px 1fr;
    gap: 16px;
    align-items: start;
  }
  @media (max-width: 560px) {
    .top {
      grid-template-columns: 1fr;
    }
  }
  .cover {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cover-box {
    width: 100%;
    aspect-ratio: 1;
    border-radius: var(--r-md);
    overflow: hidden;
    display: grid;
    place-items: center;
    background: var(--soft);
    box-shadow: inset 0 0 0 1px var(--hairline);
    font-size: var(--fs-small);
  }
  .cover-box.faded img {
    opacity: 0.55;
  }
  .cover-box img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .cover-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .note {
    margin: 0;
    font-size: var(--fs-small);
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 12px;
    align-content: start;
  }
  .wide {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--ink-2);
  }
  input {
    width: 100%;
  }
  .lyrics {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .lyrics-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  textarea {
    width: 100%;
    resize: vertical;
    min-height: 120px;
    font: inherit;
    font-size: var(--fs-small);
    line-height: 1.5;
    color: var(--ink);
    background: var(--soft);
    border: none;
    border-radius: var(--r-md);
    padding: 12px 16px;
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  textarea::placeholder {
    color: var(--ink-3);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: var(--r-md);
  }
  .results p {
    margin: 4px;
  }
  .results-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: var(--fs-small);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 260px;
    overflow-y: auto;
  }
  .candidate {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px;
    border-radius: var(--r-sm);
    text-align: left;
  }
  .candidate:hover,
  .candidate:focus-visible {
    background: var(--soft-hover);
  }
  .candidate img,
  .thumb-blank {
    width: 48px;
    height: 48px;
    flex: none;
    border-radius: 8px;
    object-fit: cover;
    background: var(--soft);
  }
  .candidate-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    font-size: var(--fs-small);
  }
  .candidate-text > * {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badges {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .hint {
    margin: 0;
    font-size: var(--fs-small);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 4px;
  }
</style>
