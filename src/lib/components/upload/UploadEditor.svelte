<script lang="ts">
  import {
    AlertCircle,
    CheckCircle2,
    ChevronDown,
    Clapperboard,
    ExternalLink,
    FileVideo,
    Film,
    Image as Picture,
    ListChecks,
    Loader2,
    Pen,
    SlidersHorizontal,
    Sparkles,
    Upload,
    X,
  } from "lucide-svelte";
  import { tick, untrack } from "svelte";
  import { mountToViewport } from "../../actions/mountToViewport";
  import { pageVisibility } from "../../page-visibility";
  import BilibiliIcon from "../BilibiliIcon.svelte";
  import { trapFocus } from "../../focus-trap";
  import { get_static_url, invoke } from "../../invoker";
  import {
    generateEventId,
    type CoverSuggestion,
    type OverlayImage,
    type ProjectItem,
    type UploadItem,
  } from "../../interface";
  import { defaultRenderSettings, frameRateQuery, parseTimeline } from "../../project-timeline";
  import { announceTask, openTaskCenter, settleTask } from "../../stores/tasks";
  import {
    isUploadActive,
    isUploadEditable,
    parseProfile,
    projectSubtitleAss,
    saveRoomTemplate,
    sourceDescription,
    sourceLabels,
  } from "../../uploads";
  import { renderAutoCover } from "../cover/auto-cover-render";
  import CoverEditor from "../cover/CoverEditor.svelte";
  import { stickerRefUrl } from "../cover/custom-stickers";
  import { renderOverlayImages, StickerLoadError } from "../project/overlay-render";
  import TypeSelect from "../TypeSelect.svelte";
  import TagInput from "../TagInput.svelte";
  import RenderSettingsFields from "../RenderSettingsFields.svelte";

  interface Props {
    upload: UploadItem;
    accounts: { uid: string; name: string }[];
    /** Reload `upload` from the backend. */
    onChanged: () => Promise<void>;
    onClose: () => void;
  }

  let { upload, accounts, onChanged, onClose }: Props = $props();
  const pageVisible = pageVisibility();

  // An id of zero is a source preview, never a persisted draft.
  let base = $state.raw(untrack(() => upload));
  let profile = $state(untrack(() => parseProfile(base.profile)));
  let fallbackTid: number | undefined = $state();
  let uid = $state(untrack(() => base.uid));
  let danmu = $state(untrack(() => base.danmu));
  let renderSettings = $state(untrack(() => ({ ...(base.render_settings ?? defaultRenderSettings()) })));
  type Covers = { cover: string; cover43: string };
  let covers: Covers | null = $state.raw(null);
  let saving = $state(false);
  let confirmClose = $state(false);
  let saveError = $state("");
  let posting = $state(false);
  let generatingCover = $state(false);
  // Bumped when the AI replaces the cover, rebuilding the manual editor so
  // it re-initializes from the new picture (its own state is kept across
  // plain open/close, which must not reset an edit in progress).
  let coverEditorKey = $state(0);
  let showCoverEditor = $state(false);
  let coverUrl = $state("");
  let coverFailed = $state(false);
  let videoUrl = $state("");
  let titleInput: HTMLInputElement | undefined = $state();
  let closeButton: HTMLButtonElement | undefined = $state();

  let editable = $derived(isUploadEditable(upload));
  let busy = $derived(saving || posting);
  let originalProfile = $derived(parseProfile(base.profile));
  let originalSettings = $derived(base.render_settings ?? defaultRenderSettings());
  let settingsChanged = $derived(JSON.stringify(renderSettings) !== JSON.stringify(originalSettings));
  let dirty = $derived(uid !== base.uid || JSON.stringify(profile) !== JSON.stringify(originalProfile)
    || danmu !== base.danmu || settingsChanged || covers !== null);
  // Turning it off stays possible even when there is nothing to burn.
  let canToggleDanmu = $derived(
    editable && !busy && (upload.danmu_available || danmu),
  );
  let active = $derived(isUploadActive(upload));
  let blocker = $derived(
    upload.source_state === "missing"
      ? "来源已不存在"
      : !profile.title.trim()
        ? "请填写标题"
        : !uid
          ? "请选择投稿账号"
          : "",
  );
  let SourceIcon = $derived(
    upload.source === "material" ? Film : upload.source === "project" ? Clapperboard : FileVideo,
  );
  let description = $derived(sourceDescription({
    ...upload, danmu,
    source_state: upload.source === "material" && upload.source_state !== "missing"
      ? (danmu && upload.danmu_available ? "render" : "material") : upload.source_state,
  }));

  $effect(() => {
    const cover = covers?.cover ?? base.display_cover;
    coverFailed = false;
    if (!cover || cover.startsWith("data:image/jpeg;base64,")) {
      coverUrl = cover || "";
      return;
    }
    let current = true;
    void get_static_url("output", cover).then((url) => { if (current) coverUrl = url; });
    return () => { current = false; };
  });

  $effect(() => {
    const video = upload.video;
    if (!video) {
      videoUrl = "";
      return;
    }
    void get_static_url(video.base, video.path).then((url) => (videoUrl = url));
  });

  // Refresh clean editors, never replace an uncommitted edit (including a cover).
  $effect(() => {
    const remote = upload;
    untrack(() => {
      if (remote.id && !dirty && !busy && !showCoverEditor && !generatingCover) adopt(remote);
    });
  });

  function adopt(item: UploadItem) {
    base = item;
    uid = item.uid;
    profile = parseProfile(item.profile);
    danmu = item.danmu;
    renderSettings = { ...(item.render_settings ?? defaultRenderSettings()) };
    covers = null;
  }

  function draftSource() {
    return { source: base.source, sourceId: base.source_id, file: base.file };
  }

  async function persist() {
    const snapshot = $state.snapshot(profile);
    snapshot.tid = fallbackTid ?? snapshot.tid;
    const stored = await invoke<UploadItem>("save_upload_draft", { edit: {
      id: base.id, source: base.id ? null : draftSource(), expectedAt: base.updated_at,
      uid, profile: snapshot,
      danmu: danmu !== base.danmu ? { base: base.danmu, value: danmu } : null,
      renderSettings: settingsChanged ? { base: originalSettings, value: $state.snapshot(renderSettings) } : null,
      covers: covers ?? (!base.id && base.display_cover?.startsWith("data:image/jpeg;base64,")
        ? { cover: base.display_cover } : null),
    } });
    adopt(stored);
    try {
      saveRoomTemplate(stored.room_id, snapshot);
    } catch (error) {
      console.warn("草稿已保存，但无法缓存房间投稿模板：", error);
    }
    return stored.id;
  }

  function setTags(tags: string[]) {
    profile.tag = tags.join(",");
  }

  async function saveDraft() {
    if (!editable || busy || generatingCover) return;
    saving = true;
    confirmClose = false;
    saveError = "";
    try {
      await persist();
      onClose();
      void onChanged().catch(console.error);
    } catch (error) {
      saveError = String(error).replace(/^.*?Failed to invoke save_upload_draft:\s*(?:Error:\s*)?/s, "");
    } finally {
      saving = false;
    }
  }

  async function saveCover(images: Covers) {
    covers = images;
  }

  /** AI 挑一帧画面并排上标题，生成即替换当前封面；不满意可再手动编辑。 */
  async function generateCover() {
    const title = profile.title.trim();
    if (!editable || busy || generatingCover || !title) return;
    generatingCover = true;
    try {
      const suggestion = await invoke<CoverSuggestion>("suggest_upload_cover", {
        id: base.id, source: base.id ? null : draftSource(), profile: $state.snapshot(profile),
      });
      covers = await renderAutoCover(suggestion, title);
      // The AI cover replaced the file underneath the manual editor: bump
      // the key so reopening rebuilds it from the new picture instead of
      // keeping the pre-AI composition that would overwrite the result.
      coverEditorKey++;
      if (suggestion.warning) {
        alert(`已用默认画面生成封面：${suggestion.warning}`);
      }
    } catch (error) {
      alert("生成封面失败：" + (error instanceof Error ? error.message : error));
    } finally {
      generatingCover = false;
    }
  }

  async function post() {
    // Not while a cover generation runs: it saves its cover after the
    // upload is claimed, which is refused, so the old cover would publish.
    if (!editable || blocker || busy || generatingCover)
      return;
    posting = true;
    const eventId = generateEventId();
    try {
      const id = await persist();
      // A project's video is rendered when posting, with its subtitles and
      // its texts and stickers as its editor draws them.
      let subtitleAss: string | null = null;
      let overlays: OverlayImage[] | null = null;
      if (upload.source === "project") {
        const project = await invoke<ProjectItem>("get_project", { id: upload.source_id });
        const timeline = parseTimeline(project.timeline);
        // Its subtitles on the frames the render has, as its editor puts them.
        const fps = timeline.clips.length
          ? await invoke<number>("get_project_frame_rate", frameRateQuery(timeline))
          : 0;
        subtitleAss = projectSubtitleAss(project, fps);
        try {
          overlays = await renderOverlayImages(timeline, stickerRefUrl);
        } catch (error) {
          throw error instanceof StickerLoadError
            ? new Error("切片中有贴纸没有加载出来，请在切片编辑窗口中删除后再投稿")
            : error;
        }
      }
      announceTask({
        id: eventId,
        task_type: "upload",
        metadata: JSON.stringify({ upload_id: id, title: profile.title }),
      });
      await invoke("post_upload", { eventId, id, subtitleAss, overlays });
      openTaskCenter(eventId);
      onClose();
      void onChanged().catch(console.error);
    } catch (error) {
      alert("投稿失败：" + (error instanceof Error ? error.message : error));
    } finally {
      posting = false;
      settleTask(eventId);
    }
  }

  function close() {
    if (busy || generatingCover) return;
    if (dirty) confirmClose = true;
    else onClose();
  }

  async function continueEditing() {
    confirmClose = false;
    await tick();
    closeButton?.focus();
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (pageVisible() && event.key === "Escape" && !event.defaultPrevented && !event.isComposing && !showCoverEditor) {
      event.preventDefault();
      if (confirmClose) void continueEditing();
      else {
        // Commit the tag input's pending text before checking for edits.
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
        close();
      }
    }
  }}
/>

<!-- Above full-window editors such as the clip editor (z-1000). -->
<div use:mountToViewport hidden={!pageVisible()} inert={!pageVisible()}>
<div class="ui-modal-backdrop z-[1100] p-2 sm:p-5">
  <div
    use:trapFocus={{ active: pageVisible(), initial: () => (editable ? titleInput : closeButton) }}
    class="ui-modal upload-sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="upload-editor-title"
  >
    <div class="sheet-head" inert={confirmClose}>
      <h3 id="upload-editor-title">
        <BilibiliIcon class="h-6 w-6 text-accent-fg" />
        {editable ? "视频投稿" : "投稿详情"}
      </h3>
      {#if editable && dirty}<span class="draft-marker">未保存</span>{/if}
      <button
        type="button"
        class="ui-btn ui-btn-icon ui-btn-ghost ml-auto"
        aria-label="关闭"
        disabled={busy || generatingCover}
        bind:this={closeButton}
        onclick={close}
      >
        <X class="h-4 w-4" />
      </button>
    </div>

    <div class="min-h-0 overflow-y-auto custom-scrollbar-light" inert={confirmClose}>
      {#if upload.status === "posted"}
        <div class="flex items-center gap-2.5 rounded-[10px] border border-success/40 bg-success-soft px-3.5 py-2.5 text-[12px] text-success-fg">
          <CheckCircle2 class="h-4 w-4 shrink-0" />
          <span class="min-w-0 flex-1">已投稿，B 站稿件审核通过后即可观看。</span>
          <a
            href={`https://www.bilibili.com/video/${upload.bvid}`}
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center gap-1 font-medium hover:underline"
          >
            {upload.bvid}
            <ExternalLink class="h-3 w-3" />
          </a>
        </div>
      {:else if active}
        <div class="flex items-center gap-2.5 rounded-[10px] border border-accent/40 bg-accent-soft px-3.5 py-2.5 text-[12px] text-accent-fg">
          <Loader2 class="h-4 w-4 shrink-0 animate-spin" />
          <span class="min-w-0 flex-1">
            {upload.status === "exporting" ? "正在生成切片视频，完成后会自动投稿。" : "正在投稿，关闭窗口不会中断。"}
          </span>
          <button type="button" class="ui-btn ui-btn-sm" onclick={() => openTaskCenter()}>
            <ListChecks class="h-3.5 w-3.5" />
            查看进度
          </button>
        </div>
      {:else if upload.status === "failed" && upload.message}
        <div class="flex items-start gap-2.5 rounded-[10px] border border-danger/40 bg-danger-soft px-3.5 py-2.5 text-[12px] text-danger-fg">
          <AlertCircle class="mt-px h-4 w-4 shrink-0" />
          <span class="min-w-0 flex-1 break-words">上次投稿失败：{upload.message}</span>
        </div>
      {/if}

      <div class="sheet-columns">
        <aside class="media-column" aria-label="视频来源与封面">
          <section>
            <h4 class="field-label">视频封面</h4>
            <div class="cover-frame">
              {#if coverUrl && !coverFailed}
                <img src={coverUrl} alt="视频封面" onerror={() => (coverFailed = true)} />
              {:else}
                <div class="empty-frame">
                  <Picture size={28} strokeWidth={1.3} />
                  <span>尚未设置封面</span>
                </div>
              {/if}
            </div>
            {#if editable}
              <div class="cover-tools">
                <button type="button" class="ui-btn ui-btn-sm ui-btn-ghost"
                  disabled={busy || generatingCover} onclick={() => (showCoverEditor = true)}>
                  <Pen size={13} /> 编辑封面
                </button>
                <button type="button" class="ui-btn ui-btn-sm ui-btn-ghost"
                  disabled={busy || generatingCover || !profile.title.trim()}
                  title={profile.title.trim() ? "AI 挑选画面并排上标题" : "先填写标题再生成封面"}
                  onclick={generateCover}>
                  {#if generatingCover}
                    <Loader2 class="h-3 w-3 animate-spin" />
                  {:else}
                    <Sparkles class="h-3 w-3" />
                  {/if}
                  {generatingCover ? "生成中…" : "AI 生成封面"}
                </button>
              </div>
            {/if}
          </section>

          <section class="source-block">
            <h4 class="field-label"><SourceIcon size={15} />{sourceLabels[upload.source]}来源</h4>
            <p class="break-words text-sm font-semibold" title={upload.source_title}>
              {upload.source_title || "已删除"}
            </p>
            {#if upload.source === "file" && upload.file !== upload.source_title}
              <p class="break-all font-mono text-[11px] text-fg-muted">{upload.file}</p>
            {/if}
            <p class="text-[11px] leading-relaxed {upload.source_state === 'missing' ? 'text-danger-fg' : 'text-fg-muted'}">
              {description}
            </p>
              {#if upload.source !== "file" && upload.source_state !== "missing" && !upload.danmu_embedded}
                <label
                  class="flex items-start justify-between gap-3 border-t border-line pt-2.5 {canToggleDanmu
                    ? 'cursor-pointer'
                    : ''}"
                >
                  <span class="min-w-0">
                    <span class="block text-[12px] font-medium text-fg">压制弹幕</span>
                    <span class="ui-hint block">
                      {!upload.danmu_available
                        ? upload.source === "project"
                          ? "切片使用的素材都没有保存弹幕"
                          : "这个素材没有保存弹幕"
                        : upload.source === "project"
                          ? "与切片导出设置中的弹幕选项同步"
                          : "开启后投稿压制了弹幕的视频"}
                    </span>
                  </span>
                  <span class="ui-switch mt-0.5">
                    <input
                      type="checkbox"
                      bind:checked={danmu}
                      disabled={!canToggleDanmu}
                    />
                    <span></span>
                  </span>
                </label>
              {/if}
          </section>
          {#if (upload.source === "project" || (upload.source === "material" && danmu && upload.danmu_available)) && upload.source_state !== "missing"}
            <details class="media-options">
              <summary><SlidersHorizontal size={14} />压制设置 <ChevronDown class="disclosure-arrow" size={14} /></summary>
              <div class="mt-3 space-y-2">
                <RenderSettingsFields
                  bind:value={renderSettings}
                  disabled={!editable || busy}
                  layout="rows"
                />
                <p class="ui-hint">{upload.source === "project" ? "与此切片的导出设置同步。" : "投稿压制弹幕时使用这些设置。"}</p>
              </div>
            </details>
          {/if}
        </aside>

        <fieldset
          class="copy-column"
          disabled={!editable || busy}
        >
          <label class="title-field">
            <span class="field-label">视频标题 <span class="char-count" aria-hidden="true">{profile.title.length} / 80</span></span>
            <input
              type="text"
              bind:this={titleInput}
              bind:value={profile.title}
              placeholder="输入视频标题"
              maxlength="80"
              class="headline-input"
            />
          </label>
          <div>
            <span class="field-label">视频分区</span>
            <TypeSelect bind:value={profile.tid} bind:fallback={fallbackTid} disabled={!editable || busy} />
          </div>
          <label class="block">
            <span class="field-label">视频简介</span>
            <textarea bind:value={profile.desc} placeholder="介绍这段视频…" rows="3" class="ui-input w-full resize-y"></textarea>
          </label>
          <div>
            <span class="field-label">标签</span>
            <TagInput
              value={[...new Set(profile.tag.split(",").map((tag) => tag.trim()).filter(Boolean))]}
              onChange={setTags}
              hint=""
              disabled={!editable || busy}
            />
          </div>
          <details class="dynamic-options" open={!!base.id && !!originalProfile.dynamic}>
            <summary>投稿动态 <span class="text-xs font-normal text-fg-muted">选填</span><ChevronDown class="disclosure-arrow" size={14} /></summary>
            <textarea aria-label="投稿动态" bind:value={profile.dynamic} placeholder="输入动态内容" rows="2" class="ui-input mt-2 w-full resize-y"></textarea>
          </details>
        </fieldset>
      </div>
    </div>

    {#if confirmClose}
      <div class="decision-bar" role="alertdialog" aria-modal="true" aria-labelledby="draft-close-title" aria-describedby="draft-close-detail" use:trapFocus>
        <div>
          <h4 id="draft-close-title">保存为草稿再离开？</h4>
          <p id="draft-close-detail">不保存，本次修改将丢失。</p>
        </div>
        <div class="decision-actions">
          <button type="button" class="ui-btn ui-btn-ghost" onclick={continueEditing}>继续编辑</button>
          <button type="button" class="ui-btn" onclick={onClose}>不保存</button>
          <button type="button" class="ui-btn ui-btn-primary" disabled={!editable} onclick={saveDraft}>保存草稿</button>
        </div>
      </div>
    {:else}
      {#if saveError}<p role="alert" class="save-error"><AlertCircle size={15} />草稿未保存：{saveError}</p>{/if}
      <div class="publish-bar">
        <div class="account-block">
          <label><span>投稿账号</span>
            <select bind:value={uid} disabled={!editable || busy || accounts.length === 0} class="ui-input">
              <option value="">{accounts.length ? "选择账号" : "未登录"}</option>
              {#each accounts as account (account.uid)}
                <option value={account.uid}>{account.name}</option>
              {/each}
              {#if uid && !accounts.some(account => account.uid === uid)}
                <option value={uid}>{uid}</option>
              {/if}
            </select>
          </label>
          {#if editable && accounts.length === 0}<p>可先存草稿，投稿前请在「账号」页登录。</p>{/if}
        </div>
        {#if editable}<div class="publish-actions">
          <button type="button" class="ui-btn" disabled={busy || generatingCover} onclick={saveDraft}>{saving ? "保存中…" : "保存草稿"}</button>
          <button
            type="button"
            class="ui-btn ui-btn-primary"
            disabled={Boolean(blocker) || busy || generatingCover}
            title={blocker || undefined}
            onclick={post}
          >
            {#if posting}
              <Loader2 class="h-3.5 w-3.5 animate-spin" />
            {:else}
              <Upload class="h-3.5 w-3.5" />
            {/if}
            {upload.status === "failed" ? "重新投稿" : "投稿"}
          </button>
        </div>{/if}
      </div>
    {/if}
  </div>
</div>

{#key coverEditorKey}
  <CoverEditor
    bind:show={showCoverEditor}
    video={videoUrl ? { file: videoUrl } : null}
    image={coverUrl}
    title={profile.title}
    onSave={saveCover}
  />
{/key}
</div>

<style>
  .upload-sheet {
    width: min(884px, 100%);
    max-height: min(90dvh, 820px);
  }
  .sheet-head, .publish-bar, .decision-bar {
    display: flex;
    flex: none;
    align-items: center;
    gap: 12px;
    padding: 16px 24px;
  }
  .sheet-head { border-bottom: 1px solid rgb(var(--c-line)); }
  .sheet-head h3, .field-label, summary, .account-block label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sheet-head h3 { font-size: 18px; font-weight: 650; }
  .draft-marker, .char-count, .account-block p, .decision-bar p {
    font-size: 12px;
    color: rgb(var(--c-fg-muted));
  }
  .draft-marker { border-left: 1px solid rgb(var(--c-line-strong)); padding-left: 12px; }
  .sheet-columns { display: grid; grid-template-columns: minmax(0, 36%) minmax(0, 1fr); }
  .media-column, .copy-column { min-width: 0; padding: 24px; }
  .media-column { background: rgb(var(--c-canvas)); border-right: 1px solid rgb(var(--c-line)); }
  .copy-column { display: grid; align-content: start; gap: 16px; }
  .copy-column :global(.ui-input), .account-block select { font-size: 13px; }
  .field-label { margin-bottom: 8px; font-size: 13px; font-weight: 600; }
  .cover-frame {
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: 4px;
    background: #273343;
    color: #dce4ef;
  }
  .cover-frame img { width: 100%; height: 100%; object-fit: cover; }
  .empty-frame {
    display: grid;
    align-content: center;
    justify-items: center;
    gap: 8px;
    height: 100%;
    font-size: 12px;
    background-image: linear-gradient(to right, transparent calc(100% - 1px), #ffffff12 0),
      linear-gradient(to bottom, transparent calc(100% - 1px), #ffffff12 0);
    background-size: 33.333% 33.333%;
  }
  .cover-tools { display: flex; justify-content: space-between; gap: 4px; padding-top: 6px; }
  .source-block, .media-options { border-top: 1px solid rgb(var(--c-line-strong)); padding-top: 16px; margin-top: 16px; }
  .source-block > p + p, .source-block label { margin-top: 5px; }
  summary { width: 100%; cursor: pointer; font-size: 13px; font-weight: 600; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible { outline: 2px solid rgb(var(--c-accent)); outline-offset: 3px; }
  summary :global(.disclosure-arrow) { margin-left: auto; color: rgb(var(--c-fg-muted)); }
  details[open] > summary :global(.disclosure-arrow) { transform: rotate(180deg); }
  .char-count { margin-left: auto; font-variant-numeric: tabular-nums; font-weight: normal; }
  .headline-input {
    width: 100%; min-width: 0; padding: 2px 0 10px;
    border: 0; border-bottom: 1px solid rgb(var(--c-line-strong)); border-radius: 0;
    background: transparent; color: rgb(var(--c-fg));
    font: 600 24px/1.35 system-ui, sans-serif;
  }
  .headline-input:focus { border-color: rgb(var(--c-accent-fg)); box-shadow: 0 1px rgb(var(--c-accent-fg)); outline: none; }
  .headline-input::placeholder { color: rgb(var(--c-fg-muted)); font-weight: 400; }
  .dynamic-options { border-top: 1px solid rgb(var(--c-line)); padding-top: 12px; }
  .publish-bar, .decision-bar { justify-content: space-between; border-top: 1px solid rgb(var(--c-line)); }
  .account-block { min-width: 0; }
  .account-block label { font-size: 12px; font-weight: 600; }
  .account-block select { max-width: min(220px, 60%); }
  .account-block p, .decision-bar p { margin-top: 3px; line-height: 1.5; }
  .publish-actions, .decision-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .publish-actions .ui-btn, .decision-actions .ui-btn { min-height: 36px; padding-inline: 15px; }
  .upload-sheet .ui-btn-primary { background-color: #0066d6; }
  .upload-sheet .ui-btn-primary:hover:not(:disabled) { background-color: #0056b5; }
  .decision-bar { background: rgb(var(--c-accent-soft)); }
  .decision-bar h4 { font-size: 16px; font-weight: 600; }
  .save-error { display: flex; align-items: baseline; gap: 7px; padding: 9px 24px; color: rgb(var(--c-danger-fg)); background: rgb(var(--c-danger-soft)); font-size: 13px; overflow-wrap: anywhere; }

  @media (max-width: 680px) {
    .upload-sheet { max-height: calc(100dvh - 16px); }
    .sheet-columns { grid-template-columns: minmax(0,1fr); }
    .sheet-head, .media-column, .copy-column, .publish-bar, .decision-bar { padding: 14px 16px; }
    .media-column {
      display: grid; grid-template-columns: minmax(0, 44%) minmax(0,1fr);
      gap: 10px 16px; align-items: start;
      border-right: 0; border-bottom: 1px solid rgb(var(--c-line));
    }
    .source-block { border: 0; padding: 0; margin: 0; }
    .media-options { grid-column: 1 / -1; margin: 0; padding-top: 10px; }
    .cover-tools { flex-wrap: wrap; justify-content: flex-start; gap: 0; }
    .cover-tools .ui-btn { padding-inline: 3px; }
    .publish-bar, .decision-bar { align-items: start; flex-wrap: wrap; }
    .publish-actions, .decision-actions { margin-left: auto; }
    .headline-input { font-size: 21px; }
  }
</style>
