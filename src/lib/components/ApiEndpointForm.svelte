<script lang="ts">
  import { connectEndpoint, ENDPOINT, forgetEndpoint } from "../web-endpoint";

  let address = $state(ENDPOINT);
  let connecting = $state(false);
  let error = $state("");
  const currentSite = new URL(".", window.location.href).href.replace(/\/$/, "");

  async function connect(event: SubmitEvent) {
    event.preventDefault();
    if (connecting) return;
    error = "";
    connecting = true;
    try {
      await connectEndpoint(address);
      // Reload the same page (including preview/editor query parameters).
      // The invoker and event socket will both start with the new endpoint.
      window.location.reload();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      connecting = false;
    }
  }
</script>

<form onsubmit={connect} novalidate class="space-y-2">
  <label for="api-endpoint" class="ui-label">API 服务器地址</label>
  <div class="flex flex-wrap items-center gap-2">
    <input
      id="api-endpoint"
      type="url"
      required
      spellcheck="false"
      autocapitalize="none"
      class="ui-input min-w-0 flex-1 basis-48"
      placeholder={currentSite}
      bind:value={address}
      oninput={() => error = ""}
      disabled={connecting}
      aria-describedby={error ? "api-endpoint-error" : undefined}
    />
    <button type="submit" class="ui-btn ui-btn-primary" disabled={connecting || !address.trim() || address === ENDPOINT}>
      {connecting ? "正在连接…" : ENDPOINT ? "保存并连接" : "连接并进入"}
    </button>
  </div>
  {#if ENDPOINT}
    <button type="button" class="ui-btn ui-btn-ghost ui-btn-sm" disabled={connecting} onclick={() => {
      forgetEndpoint();
      window.location.reload();
    }}>清除地址并返回连接页</button>
  {:else}
    <button type="button" class="text-[12px] text-accent-fg hover:underline" disabled={connecting} onclick={() => {
      address = currentSite;
      error = "";
    }}>使用当前站点地址</button>
  {/if}
  {#if error}<p id="api-endpoint-error" class="text-[12px] text-danger-fg" role="alert">{error}</p>{/if}
</form>
