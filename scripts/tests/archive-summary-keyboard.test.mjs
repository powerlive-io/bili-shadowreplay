import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { recordingStorage } from "./recording-storage-fixture.mjs";

const source = await readFile(new URL("../../src/lib/components/ArchiveSummaryModal.svelte", import.meta.url), "utf8");
const script = source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const { outputText } = ts.transpileModule(`${script}
  export { handleKeydown, generate };
  export function isOpen() { return showModal; }
  export function show(record) {
    showModal = true;
    archive = record;
    currentKey = archiveKey(record);
  }
  export function snapshot() { return { summary, error, actionLoading, llmConfigError }; }
`, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });

async function fixture(invoke = () => { throw new Error("Unexpected API request"); }) {
  let visible = true;
  let effect, cleanup;
  const statuses = [], polls = new Set();
  const context = vm.createContext({
    $state: value => value,
    $derived: value => value,
    $bindable: value => value,
    $effect(fn) { effect = fn; },
    $props: () => ({ showModal: true, onUpdated: status => statuses.push(status) }),
    setTimeout(fn) { polls.add(fn); return fn; },
    clearTimeout(fn) { polls.delete(fn); },
  });
  const dependencies = {
    "../recording-storage": recordingStorage,
    "../archives": { archiveKey: record => `${record.platform}:${record.room_id}:${record.live_id}` },
    svelte: { untrack: fn => fn() },
    "../page-visibility": { pageVisibility: () => () => visible },
    "../invoker": { invoke },
  };
  const module = new vm.SourceTextModule(outputText, { context });
  await module.link(name => {
    const exports = dependencies[name];
    assert.ok(exports, `Unexpected dependency: ${name}`);
    return new vm.SyntheticModule(Object.keys(exports), function () {
      for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
    }, { context });
  });
  await module.evaluate();
  const handlers = module.namespace;
  return {
    handlers, statuses, polls, setVisible: value => { visible = value; },
    show(live_id) {
      cleanup?.();
      handlers.show({ platform: "test", room_id: "room", live_id });
      cleanup = effect();
    },
  };
}

test("late results cannot write to another archive or a reopened session of the same archive", async () => {
  const oldRead = Promise.withResolvers(), oldConfig = Promise.withResolvers(), generate = Promise.withResolvers();
  const reads = [oldRead.promise, { live_id: "B", status: "success" }, { live_id: "A", status: "success" }];
  const configs = [oldConfig.promise, {}, {}];
  const f = await fixture(command => {
    if (command === "get_archive_summary") return reads.shift();
    if (command === "get_llm_config") return configs.shift();
    assert.equal(command, "generate_archive_summary");
    return generate.promise;
  });
  const settle = () => new Promise(resolve => setImmediate(resolve));
  f.show("A");
  f.show("B");
  await settle();
  const oldGenerate = f.handlers.generate(false);
  f.handlers.handleKeydown(keyEvent());
  f.show("A");
  await settle();
  oldRead.resolve({ live_id: "A", status: "processing" });
  oldConfig.reject(new Error("stale config"));
  generate.resolve({ id: "old B task" });
  await oldGenerate;
  await settle();
  const view = f.handlers.snapshot();
  assert.equal(view.summary.live_id, "A");
  assert.equal(view.summary.status, "success");
  assert.equal(view.llmConfigError, false);
  assert.equal(view.actionLoading, false);
  assert.deepEqual(f.statuses.map(s => s.live_id), ["B", "A"]);
  assert.equal(f.polls.size, 0);
});

function keyEvent(changes = {}) {
  return {
    key: "Escape", defaultPrevented: false, isComposing: false,
    preventCalls: 0, stopCalls: 0,
    preventDefault() { this.defaultPrevented = true; this.preventCalls++; },
    stopImmediatePropagation() { this.stopCalls++; },
    ...changes,
  };
}

test("a hidden summary preserves Escape for the active page's dialog", async () => {
  const f = await fixture();
  f.setVisible(false);
  const event = keyEvent();
  f.handlers.handleKeydown(event);
  assert.equal(f.handlers.isOpen(), true);
  assert.equal(event.preventCalls, 0);
  assert.equal(event.stopCalls, 0);
});

test("returning to the summary's page restores Escape dismissal", async () => {
  const f = await fixture();
  f.setVisible(false);
  f.handlers.handleKeydown(keyEvent());
  f.setVisible(true);
  const event = keyEvent();
  f.handlers.handleKeydown(event);
  assert.equal(f.handlers.isOpen(), false);
  assert.equal(event.preventCalls, 1);
  assert.equal(event.stopCalls, 1);
  const closed = keyEvent();
  f.handlers.handleKeydown(closed);
  assert.equal(closed.preventCalls, 0);
  assert.equal(closed.stopCalls, 0);
});

test("a visible summary leaves composed, already handled, and other keys untouched", async () => {
  const f = await fixture();
  for (const changes of [{ isComposing: true }, { defaultPrevented: true }, { key: "Enter" }]) {
    const event = keyEvent(changes);
    f.handlers.handleKeydown(event);
    assert.equal(f.handlers.isOpen(), true);
    assert.equal(event.preventCalls, 0);
    assert.equal(event.stopCalls, 0);
  }
});
