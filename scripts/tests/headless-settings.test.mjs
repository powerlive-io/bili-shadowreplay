import assert from "node:assert/strict";
import { before, test } from "node:test";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { JSDOM, VirtualConsole } from "jsdom";
import { build } from "vite";

// Use the same entry gate as all three windows. Keep dynamic imports split:
// importing the settings page eagerly would also start its event transport.
let chunks;
before(async () => {
  const entry = "virtual:settings-page";
  const result = await build({
    configFile: false, publicDir: false, logLevel: "silent",
    resolve: { conditions: ["browser", "module", "svelte"] },
    plugins: [svelte(), {
      name: "settings-fixture", enforce: "pre",
      resolveId(id) {
        if (id === entry || id === "socket.io-client") return `\0${id}`;
      },
      load(id) {
        if (id === `\0${entry}`) return `
          import { mountPage } from "/src/lib/mount-page.ts";
          mountPage(() => import("/src/page/Setting.svelte"));
        `;
        if (id === "\0socket.io-client") return `
          export function io(url, options) {
            window.sockets.push([url, options.path]);
            return { on() {}, onAny() {} };
          }`;
      },
    }],
    build: {
      write: false, minify: false, modulePreload: false, cssCodeSplit: false,
      rollupOptions: { input: entry, output: { format: "cjs", entryFileNames: "entry.js", chunkFileNames: "[name].js" } },
    },
  });
  chunks = new Map(result.output.filter(file => file.type === "chunk").map(file => [file.fileName, file.code]));
});

const settle = () => new Promise(resolve => setImmediate(resolve));
async function page(t, { endpoint, desktop = false, offline = false } = {}) {
  const dom = new JSDOM('<!doctype html><div id="app"></div>', {
    url: "http://localhost:4321/", runScripts: "outside-only",
    // jsdom reports location.reload() as unimplemented; simulate a new visit
    // with the persisted address in the tests below.
    virtualConsole: new VirtualConsole().on("jsdomError", err => {
      if (!err.message.includes("navigation")) throw err;
    }),
  });
  t.after(() => dom.window.close());
  const w = dom.window;
  w.sockets = [];
  const calls = [];
  const config = {
    status_check_interval: 42, output: "./output", whisper_model: "old.bin", subtitle_generator_type: "whisper",
    auto_generate: { enabled: false }, llm: { provider: "openai", endpoint: "https://example.test/v1", api_key: "", model: "" },
    danmu_ass_options: { font_size: 36, opacity: 0.8 },
  };
  if (endpoint) w.localStorage.setItem("endpoint", endpoint);
  function command(name, args) {
    if (name === "get_account_count") {
      if (offline) throw new Error("offline");
      return 0;
    }
    if (name === "get_config") {
      if (offline) throw new Error("offline");
      return config;
    }
    if (name === "get_mcp_server_status") return { enabled: false, running: false, port: 23456, url: "http://127.0.0.1:23456/mcp" };
    if (name === "update_whisper_model") config.whisper_model = args.whisperModel;
    if (name === "get_powerlive_status") return { logged_in: false, pending: false };
  }
  w.fetch = async (url, options) => {
    calls.push(url);
    const data = command(url.split("/").pop(), JSON.parse(options.body));
    return { ok: true, json: async () => w.JSON.parse(JSON.stringify({ code: 0, data })) };
  };
  if (desktop) w.__TAURI_INTERNALS__ = { invoke: async (name, args) => command(name, args) };
  const modules = new Map();
  function requireChunk(path) {
    const name = path.replace(/^\.\//, "");
    if (!modules.has(name)) {
      const module = { exports: {} };
      modules.set(name, module);
      assert.ok(chunks.has(name), `unknown chunk ${name}`);
      new w.Function("module", "exports", "require", chunks.get(name))(module, module.exports, requireChunk);
    }
    return modules.get(name).exports;
  }
  requireChunk("entry.js");
  await settle();
  const row = id => w.document.querySelector(`[data-setting="${id}"]`);
  return { w, calls, config, row, reconnect: () => { offline = false; } };
}

test("first visit requires explicit configuration and does not start application requests or sockets", async t => {
  const { w, calls, row, reconnect } = await page(t, { offline: true });
  assert.ok(w.document.querySelector("#connect-title"));
  assert.equal(row("theme"), null);
  const input = w.document.querySelector("#api-endpoint");
  const form = input.form;
  const submit = form.querySelector("[type=submit]");
  assert.equal(input.value, "");
  assert.equal(submit.disabled, true);
  input.value = "file:///tmp/bsr";
  input.dispatchEvent(new w.Event("input", { bubbles: true }));
  await settle();
  submit.click();
  await settle();
  assert.match(form.querySelector('[role="alert"]').textContent, /HTTPS/);
  assert.deepEqual(calls, []);
  assert.equal(w.sockets.length, 0);
  form.querySelector("[type=button]").click();
  await settle();
  assert.equal(input.value, "http://localhost:4321");
  submit.click();
  await settle();
  assert.match(form.querySelector('[role="alert"]').textContent, /无法连接/);
  assert.equal(w.localStorage.getItem("endpoint"), null);
  reconnect();
  submit.click();
  await settle();
  assert.deepEqual(calls, Array(2).fill("http://localhost:4321/api/get_account_count"));
  assert.equal(w.sockets.length, 0);
  const next = await page(t, { endpoint: w.localStorage.getItem("endpoint") });
  assert.ok(next.row("theme"));
  assert.equal(next.w.document.querySelector("#connect-title"), null);
});

for (const endpoint of ["http://localhost:4321", "https://bsr.example/base/"]) {
  test(`configured web settings load, show MCP status and save against ${endpoint}`, async t => {
    const { w, calls, config, row } = await page(t, { endpoint });
    const api = endpoint.replace(/\/$/, "") + "/api/";
    assert.deepEqual(calls, [api + "get_config", api + "get_mcp_server_status"]);
    const url = new URL(endpoint);
    assert.deepEqual(Array.from(w.sockets[0]), [url.origin + "/ws", url.pathname.replace(/\/$/, "") + "/socket.io"]);
    for (const id of ["recording-storage", "llm-model", "auto-subtitle", "clip-name-format", "danmu-opacity", "auto-generate"]) {
      assert.ok(row(id), `missing setting: ${id}`);
    }
    assert.equal(row("status-check-interval").querySelector("input").value, "42");
    assert.equal(row("mcp-port").querySelector("input").value, "23456");
    assert.equal(row("launch-at-login"), null);
    assert.equal(row("output-path").querySelector("button"), null);
    assert.equal(row("log-folder").querySelector("button"), null);
    assert.equal(row("notify-live-start"), null);
    const input = row("whisper-model").querySelector("input");
    assert.equal(input.value, config.whisper_model);
    input.value = "/models/test.bin";
    input.dispatchEvent(new w.Event("input", { bubbles: true }));
    await settle();
    row("whisper-model").querySelector("button").click();
    await settle();
    assert.equal(calls.at(-1), api + "update_whisper_model");
    assert.equal(config.whisper_model, "/models/test.bin");
  });
}

test("an unreachable API leaves connection settings available without editable server defaults", async t => {
  const endpoint = "http://localhost:4321";
  const { w, calls, row, reconnect } = await page(t, { endpoint, offline: true });
  assert.ok(row("theme") && row("api-endpoint"));
  assert.equal(row("webhook"), null);
  assert.equal(row("auto-generate"), null);
  const error = w.document.querySelector('[role="alert"]');
  assert.match(error.textContent, /offline/);
  reconnect();
  error.parentElement.querySelector("button").click();
  await settle();
  assert.ok(row("webhook") && row("auto-generate"));
  assert.deepEqual(calls, ["get_config", "get_config", "get_mcp_server_status"].map(name => `${endpoint}/api/${name}`));
  row("api-endpoint").querySelector("[type=button]").click();
  assert.equal(w.localStorage.getItem("endpoint"), null);
});

test("switching servers verifies the candidate before replacing the working address", async t => {
  const endpoint = "http://localhost:4321";
  const { w, row } = await page(t, { endpoint });
  const form = row("api-endpoint").querySelector("form");
  const input = form.querySelector("input");
  input.value = "https://new.example/bsr/";
  input.dispatchEvent(new w.Event("input", { bubbles: true }));
  await settle();
  const probe = w.fetch;
  w.fetch = async () => ({ ok: true, json: async () => { throw new SyntaxError("Not JSON"); } });
  form.querySelector("[type=submit]").click();
  await settle();
  assert.match(form.querySelector('[role="alert"]').textContent, /未返回 BSR API/);
  assert.equal(w.localStorage.getItem("endpoint"), endpoint);
  w.fetch = probe;
  form.querySelector("[type=submit]").click();
  await settle();
  assert.equal(w.localStorage.getItem("endpoint"), "https://new.example/bsr");
});

test("desktop keeps native actions and uses IPC instead of HTTP", async t => {
  const { calls, row } = await page(t, { desktop: true });
  assert.deepEqual(calls, []);
  assert.equal(row("api-endpoint"), null);
  assert.ok(row("launch-at-login") && row("notify-live-start") && row("llm-model"));
  for (const id of ["output-path", "log-folder", "whisper-model"]) assert.ok(row(id).querySelector("button"));
  assert.equal(row("whisper-model").querySelector("input"), null);
});
