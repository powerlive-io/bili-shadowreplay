<script lang="ts">
  import {
    AlertCircle,
    CheckCircle2,
    Clapperboard,
    ExternalLink,
    FileVideo,
    Film,
    ListChecks,
    Loader2,
    Pen,
    Sparkles,
    Upload,
    X,
  } from "lucide-svelte";
  import { untrack } from "svelte";
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
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !event.defaultPrevented && !event.isComposing && !showCoverEditor) {
      event.preventDefault();
      if (confirmClose) confirmClose = false;
      else {
        // Commit the tag input's pending text before checking for edits.
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
        close();
      }
    }
  }}
/>

<!-- Above full-window editors such as the clip editor (z-1000). -->
<div class="ui-modal-backdrop z-[1100]" inert={confirmClose}>
  <div
    use:trapFocus={{ initial: () => (editable ? titleInput : closeButton) }}
    class="ui-modal max-h-[90vh] w-[820px] max-w-full"
    role="dialog"
    aria-modal="true"
    aria-labelledby="upload-editor-title"
  >
    <div class="ui-modal-header">
      <h3 id="upload-editor-title" class="flex min-w-0 items-center gap-2">
        <Upload class="h-4 w-4 shrink-0 text-accent-fg" />
        <span class="truncate">{editable ? "编辑投稿" : "投稿详情"}</span>
      </h3>
      <button
        type="button"
        class="ui-btn ui-btn-sm ui-btn-icon ui-btn-ghost -mr-1.5"
        aria-label="关闭"
        disabled={busy || generatingCover}
        bind:this={closeButton}
        onclick={close}
      >
        <X class="h-4 w-4" />
      </button>
    </div>

    <div class="ui-modal-body custom-scrollbar-light space-y-4">
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

      <div class="grid grid-cols-[280px_minmax(0,1fr)] gap-5">
        <div class="space-y-4">
          <section class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="ui-label">视频封面</span>
              {#if editable}
                <div class="flex gap-1.5">
                  <button
                    type="button"
                    class="ui-btn ui-btn-sm"
                    disabled={busy || generatingCover || !profile.title.trim()}
                    title={profile.title.trim() ? "AI 挑选画面并排上标题" : "先填写标题再生成封面"}
                    onclick={generateCover}
                  >
                    {#if generatingCover}
                      <Loader2 class="h-3 w-3 animate-spin" />
                    {:else}
                      <Sparkles class="h-3 w-3" />
                    {/if}
                    {generatingCover ? "生成中…" : "AI 生成封面"}
                  </button>
                  <button
                    type="button"
                    class="ui-btn ui-btn-sm"
                    disabled={busy || generatingCover}
                    title={generatingCover ? "AI 封面生成中，完成后再手动编辑" : undefined}
                    onclick={() => (showCoverEditor = true)}
                  >
                    <Pen class="h-3 w-3" />
                    编辑封面
                  </button>
                </div>
              {/if}
            </div>
            <div class="aspect-video overflow-hidden rounded-[10px] border border-line bg-sunken">
              {#if coverUrl && !coverFailed}
                <img src={coverUrl} alt="视频封面" class="h-full w-full object-cover" onerror={() => (coverFailed = true)} />
              {:else}
                <div class="flex h-full items-center justify-center text-[11px] text-fg-faint">暂无封面</div>
              {/if}
            </div>
          </section>

          <section class="space-y-2">
            <span class="ui-label">投稿来源</span>
            <div class="ui-card space-y-2 bg-control/50 p-3">
              <div class="flex min-w-0 items-center gap-2">
                <SourceIcon class="h-4 w-4 shrink-0 text-fg-muted" />
                <span class="ui-badge">{sourceLabels[upload.source]}</span>
                <span class="min-w-0 truncate text-[12px] font-medium text-fg" title={upload.source_title}>
                  {upload.source_title || "已删除"}
                </span>
              </div>
              {#if upload.source === "file"}
                <p class="break-all font-mono text-[11px] text-fg-muted">{upload.file}</p>
              {/if}
              <p class="text-[11px] leading-relaxed {upload.source_state === 'missing' ? 'text-danger-fg' : 'text-fg-muted'}">
                {sourceDescription({ ...upload, danmu, source_state: upload.source === "material" && upload.source_state !== "missing" ? (danmu && upload.danmu_available ? "render" : "material") : upload.source_state })}
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
            </div>
          </section>
          {#if (upload.source === "project" || (upload.source === "material" && danmu && upload.danmu_available)) && upload.source_state !== "missing"}
            <section class="space-y-2">
              <span class="ui-label">画质、分辨率与码率</span>
              <div class="ui-card space-y-2 bg-control/50 p-3">
                <RenderSettingsFields
                  bind:value={renderSettings}
                  disabled={!editable || busy}
                />
                <p class="ui-hint">{upload.source === "project" ? "与此切片的导出设置同步。" : "投稿压制弹幕时使用这些设置。"}</p>
              </div>
            </section>
          {/if}
        </div>

        <fieldset
          class="min-w-0 space-y-3.5 disabled:opacity-70"
          disabled={!editable || busy}
        >
          <label class="block space-y-1.5">
            <span class="ui-label">标题</span>
            <input
              type="text"
              bind:this={titleInput}
              bind:value={profile.title}
              placeholder="输入视频标题"
              maxlength="80"
              class="ui-input w-full"
            />
          </label>
          <div class="space-y-1.5">
            <span class="ui-label">视频分区</span>
            <TypeSelect bind:value={profile.tid} bind:fallback={fallbackTid} disabled={!editable || busy} />
          </div>
          <label class="block space-y-1.5">
            <span class="ui-label">投稿账号</span>
            <select bind:value={uid} class="ui-input w-full">
              <option value="">选择账号</option>
              {#each accounts as account (account.uid)}
                <option value={account.uid}>{account.name}</option>
              {/each}
            </select>
            {#if accounts.length === 0}
              <span class="ui-hint block">还没有 B 站账号，请先在「账号」页登录。</span>
            {/if}
          </label>
          <label class="block space-y-1.5">
            <span class="ui-label">描述</span>
            <textarea bind:value={profile.desc} placeholder="输入视频描述" rows="3" class="ui-input w-full resize-none"></textarea>
          </label>
          <div class="space-y-1.5">
            <span class="ui-label">标签</span>
            <TagInput
              value={[...new Set(profile.tag.split(",").map((tag) => tag.trim()).filter(Boolean))]}
              onChange={setTags}
              disabled={!editable || busy}
            />
          </div>
          <label class="block space-y-1.5">
            <span class="ui-label">动态</span>
            <textarea bind:value={profile.dynamic} placeholder="输入动态内容" rows="2" class="ui-input w-full resize-none"></textarea>
          </label>
        </fieldset>
      </div>
    </div>

    <div class="ui-modal-footer justify-between">
      <span class="min-w-0 truncate text-[11px] {saveError ? 'text-danger-fg' : 'text-fg-muted'}">
        {#if saveError}
          保存失败：{saveError}
        {:else if editable && dirty}
          有未保存的修改
        {/if}
      </span>
      <div class="flex shrink-0 gap-2">
        {#if editable}
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
        {/if}
      </div>
    </div>
  </div>
</div>

{#if confirmClose}
  <div class="ui-modal-backdrop z-[1110]">
    <div class="ui-modal max-w-sm" role="alertdialog" aria-modal="true" aria-labelledby="draft-close-title" use:trapFocus>
      <div class="ui-modal-header"><h3 id="draft-close-title">保存本次修改？</h3></div>
      <div class="ui-modal-body text-sm text-fg-muted">不保存就关闭，本次修改将丢失。</div>
      <div class="ui-modal-footer">
        <button type="button" class="ui-btn" onclick={() => (confirmClose = false)}>继续编辑</button>
        <button type="button" class="ui-btn" onclick={onClose}>不保存</button>
        <button type="button" class="ui-btn ui-btn-primary" disabled={!editable} onclick={saveDraft}>保存草稿</button>
      </div>
    </div>
  </div>
{/if}

{#key coverEditorKey}
  <CoverEditor
    bind:show={showCoverEditor}
    video={videoUrl ? { file: videoUrl } : null}
    image={coverUrl}
    title={profile.title}
    onSave={saveCover}
  />
{/key}
