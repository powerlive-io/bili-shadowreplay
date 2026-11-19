<script lang="ts">
  import type { RenderSettings } from "../interface";
  import RenderSettingsFields from "./RenderSettingsFields.svelte";

  interface Props {
    encodeDanmu: boolean;
    reencode: boolean;
    renderSettings: RenderSettings;
    disabled?: boolean;
    wholeRecording?: boolean;
  }
  let {
    encodeDanmu = $bindable(), reencode = $bindable(), renderSettings = $bindable(),
    disabled = false, wholeRecording = false,
  }: Props = $props();
  const encoding = $derived(encodeDanmu || reencode);
</script>

<div class="flex items-center justify-between gap-4">
  <label class="flex items-center gap-2 text-[12px] text-fg-secondary">
    <input type="checkbox" class="ui-checkbox" checked={encoding}
      onchange={(event) => (reencode = event.currentTarget.checked)} disabled={encodeDanmu || disabled} />
    <span>重新编码</span>
  </label>
  <label class="flex cursor-pointer items-center gap-2 text-[12px] text-fg-secondary">
    <span>内嵌弹幕</span>
    <span class="ui-switch">
      <input type="checkbox" class="left-0 top-0" bind:checked={encodeDanmu} {disabled} />
      <span></span>
    </span>
  </label>
</div>
<p class="ui-hint">
  {encodeDanmu ? "内嵌弹幕需要重新编码，无法取消"
    : reencode ? "按以下设置重新编码，耗时较长"
    : wholeRecording ? "分辨率一致时直接复制；不一致时自动重新编码，统一到最大片段尺寸"
    : "直接复制原视频，保留原分辨率、编码格式和码率。"}
</p>
<RenderSettingsFields layout="columns" bind:value={renderSettings} streamCopy={!encoding}
  originalResolutionSource={wholeRecording ? "largest" : "first"} {disabled} showHint={encoding} />
