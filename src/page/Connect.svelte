<script lang="ts">
  import ApiEndpointForm from "../lib/components/ApiEndpointForm.svelte";
</script>

<svelte:head><title>连接服务器 · BiliBili ShadowReplay</title></svelte:head>

<main class="grid min-h-screen place-items-center bg-canvas p-5 text-fg">
  <section class="ui-card w-full max-w-md space-y-6 p-6" aria-labelledby="connect-title">
    <div class="space-y-3">
      <img src="./icon.svg" alt="" width="40" height="40" />
      <h1 id="connect-title" class="text-xl font-semibold">连接 ShadowReplay 服务器</h1>
      <p class="text-sm text-fg-secondary">填写运行 ShadowReplay 的服务地址。连接成功后，此浏览器会记住地址，下次直接进入。</p>
    </div>
    <ApiEndpointForm />
  </section>
</main>
