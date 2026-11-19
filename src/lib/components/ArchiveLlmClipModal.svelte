<script lang="ts">
  import { Info, X } from "lucide-svelte";
  import type { RecordItem } from "../db";
  import { trapFocus } from "../focus-trap";
  import { pageVisibility } from "../page-visibility";
  import { defaultRenderSettings } from "../project-timeline";
  import { archiveFilesAvailable, archiveStorageReason } from "../recording-storage";
  import ArchiveEncodingFields from "./ArchiveEncodingFields.svelte";
  import { generateLlmClips, llmClipRunning } from "./ArchiveLlmClipButton.svelte";

  let { archive, onclose }: { archive: RecordItem; onclose: () => void } = $props();
  const pageVisible = pageVisibility();
  // Keep the one-click action's existing embedded-danmaku default.
  let encodeDanmu = $state(true);
  let reencode = $state(false);
  let renderSettings = $state(defaultRenderSettings());
  let namePrefix = $state("");
  const available = $derived(archiveFilesAvailable(archive));
  const canStart = $derived(available && archive.length > 0 && !llmClipRunning(archive));

  function confirm() {
    if (!canStart) return;
    void generateLlmClips(archive, {
      danmu: encodeDanmu,
      renderSettings: encodeDanmu || reencode ? { ...renderSettings } : undefined,
      namePrefix: namePrefix.trim(),
    });
    onclose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!pageVisible() || event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    onclose();
  }
</script>

<div class="ui-modal-backdrop">
  <button class="absolute inset-0" type="button" aria-label="取消一键智能切片" onclick={onclose}></button>
  <div role="dialog" aria-modal="true" aria-label="一键智能切片" tabindex="-1"
    class="ui-modal relative max-h-[90vh] w-[600px] max-w-full"
    use:trapFocus={{ active: pageVisible() }} onkeydown={handleKeydown}>
    <div class="ui-modal-header">
      <div class="flex min-w-0 items-baseline gap-2.5">
        <h2 class="shrink-0">一键智能切片</h2>
        <span class="truncate text-[12px] text-fg-muted" title={archive.title}>{archive.title}</span>
      </div>
      <button class="ui-btn ui-btn-ghost ui-btn-sm ui-btn-icon" aria-label="关闭切片设置" onclick={onclose}><X class="h-4 w-4" /></button>
    </div>
    <div class="ui-modal-body custom-scrollbar-light space-y-4">
      <p class="text-[13px] leading-relaxed text-fg-secondary">
        自动按录播总结中的高光时间段剪辑，无需手动选择起止时间。每个高光会按下方设置导出为一段独立的视频素材。
      </p>
      <div role="note" class="flex items-start gap-2 rounded-[8px] bg-control p-3 text-[12px] leading-relaxed text-fg-secondary">
        <Info aria-hidden="true" class="mt-0.5 h-4 w-4 shrink-0 text-fg-muted" />
        <p>额度提示：已有可用总结时会直接复用。生成新总结时，会同时使用字幕生成额度和 LLM 额度。</p>
      </div>
      <div class="space-y-3"><ArchiveEncodingFields bind:encodeDanmu bind:reencode bind:renderSettings /></div>
      <label class="block space-y-1.5">
        <span class="ui-label">输出文件名前缀</span>
        <input class="ui-input w-full" bind:value={namePrefix} maxlength="64" placeholder="留空使用默认命名规则" />
        <span class="ui-hint block">填写前缀后，文件名按「前缀-01.mp4」「前缀-02.mp4」编号。</span>
      </label>
      {#if !available}<p role="alert" class="text-[12px] text-danger-fg">{archiveStorageReason(archive)}</p>{/if}
    </div>
    <div class="ui-modal-footer">
      <button class="ui-btn" onclick={onclose}>取消</button>
      <button class="ui-btn ui-btn-primary" disabled={!canStart} onclick={confirm}>确认并生成切片</button>
    </div>
  </div>
</div>
