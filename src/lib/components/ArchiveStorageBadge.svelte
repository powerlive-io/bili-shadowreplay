<script lang="ts">
  import type { RecordItem } from "../db";
  import { archiveStorageLabel, archiveStorageReason } from "../recording-storage";

  let { archive, showLabel = true }: { archive: RecordItem; showLabel?: boolean } = $props();
  const label = $derived(archiveStorageLabel(archive));
</script>

<span class="inline-flex max-w-full flex-wrap items-center gap-1 text-[11px]">
  {#if archive.storage_backend_id || !showLabel}
    <span class="select-text break-all text-fg-muted" title="此录播固定使用的存储，不随录制器偏好改变">{showLabel ? "存储：" : ""}{archive.storage_backend_id || "—"}</span>
  {/if}
  {#if label}
    <span class="ui-badge {archive.storage_initializing ? 'ui-badge-accent' : 'ui-badge-danger'}" title={archiveStorageReason(archive)}>{label}</span>
  {/if}
</span>
