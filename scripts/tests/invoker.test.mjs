import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";

async function loadInvoker({ tauri = false } = {}) {
  const fetches = [];
  const tauriCalls = [];
  const response = { content: "Hello", toolCalls: [] };
  const context = vm.createContext({
    window: tauri ? { __TAURI_INTERNALS__: {} } : {},
    localStorage: { getItem: () => "https://bsr.example/base/" },
    fetch: async (url, options) => {
      fetches.push({ url, ...options });
      return { ok: true, status: 200, json: async () => ({ code: 0, data: response }) };
    },
  });
  const dependencies = {
    "@tauri-apps/api/core": {
      invoke: async (command, args) => {
        tauriCalls.push({ command, args });
        return response;
      },
    },
    "@tauri-apps/plugin-http": { fetch: async () => {} },
    "@tauri-apps/plugin-shell": { open: async () => {} },
    "@tauri-apps/plugin-deep-link": { onOpenUrl: async () => {} },
    "./events": { createEventClient: () => ({ listen: async () => () => {} }) },
  };
  const source = await readFile(new URL("../../src/lib/invoker.ts", import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  });
  const module = new vm.SourceTextModule(outputText, { context });
  const linked = new Map();
  await module.link((name) => {
    const exports = dependencies[name];
    if (!exports) throw new Error(`unexpected import ${name}`);
    if (!linked.has(name)) {
      linked.set(name, new vm.SyntheticModule(Object.keys(exports), function () {
        for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
      }, { context }));
    }
    return linked.get(name);
  });
  await module.evaluate();
  return { invoke: module.namespace.invoke, fetches, tauriCalls, response };
}

test("web agent_chat sends the AgentRequest at the HTTP body root", async () => {
  const { invoke, fetches, tauriCalls, response } = await loadInvoker();
  const request = {
    messages: [
      { role: "user", content: "Describe this", parts: [{ type: "text", text: "Details" }] },
      { role: "assistant", content: "", toolCalls: [{ id: "call-1", name: "list_rooms", args: {} }] },
      { role: "tool", content: "[]", toolCallId: "call-1" },
    ],
  };
  const args = { request };
  const before = structuredClone(args);
  assert.equal(await invoke("agent_chat", args), response);
  assert.equal(fetches.length, 1);
  assert.equal(fetches[0].url, "https://bsr.example/base/api/agent_chat");
  assert.equal(fetches[0].method, "POST");
  assert.equal(fetches[0].headers["Content-Type"], "application/json");
  assert.deepEqual(JSON.parse(fetches[0].body), request);
  assert.deepEqual(args, before);
  assert.deepEqual(tauriCalls, []);
});

test("desktop agent_chat keeps the request argument wrapper", async () => {
  const { invoke, fetches, tauriCalls, response } = await loadInvoker({ tauri: true });
  const args = { request: { messages: [{ role: "user", content: "Hello" }] } };
  assert.equal(await invoke("agent_chat", args), response);
  assert.deepEqual(tauriCalls, [{ command: "agent_chat", args }]);
  assert.equal(tauriCalls[0].args, args);
  assert.deepEqual(fetches, []);
});

test("other HTTP commands preserve their complete arguments, including request", async () => {
  const { invoke, fetches } = await loadInvoker();
  const args = { request: { path: "clip.mp4" }, enabled: true };
  await invoke("other_command", args);
  assert.equal(fetches[0].url, "https://bsr.example/base/api/other_command");
  assert.deepEqual(JSON.parse(fetches[0].body), args);
});

test("HTTP commands without arguments still send an empty object", async () => {
  const { invoke, fetches } = await loadInvoker();
  await invoke("get_settings");
  assert.deepEqual(JSON.parse(fetches[0].body), {});
});
