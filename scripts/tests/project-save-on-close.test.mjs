import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = await readFile(new URL("../../src/lib/components/project/ProjectEditor.svelte", import.meta.url), "utf8");
const script = source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const parsed = ts.createSourceFile("ProjectEditor.ts", script, ts.ScriptTarget.Latest, true);
const names = ["handleCloseRequested", "saveAll", "saveInfo", "flushSave", "saveTimelineSnapshot"];
const handlers = parsed.statements.filter((node) => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
assert.equal(handlers.length, names.length);
const code = ts.transpileModule(handlers.map((node) => node.getText(parsed)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText;

function fixture() {
  const alerts = [];
  const state = vm.createContext({
    saveTimer: null, saving: null, saveState: "pending", resolutionSaving: null,
    timeline: { clips: [] }, syncedTimeline: "{}", parseTimeline: JSON.parse,
    projectId: 1, project: { title: "old" }, title: "new",
    applyProjectView: () => false, set_title: async () => {},
    endEdit() {}, finishEditing() {}, settleOverlayEdit() {},
    alert: (message) => alerts.push(message),
  });
  vm.runInContext(code, state);
  // Tauri awaits the handler before deciding whether to destroy the window.
  async function requestClose() {
    let prevented = false;
    await state.handleCloseRequested({ preventDefault() { prevented = true; } });
    return !prevented;
  }
  return { state, requestClose, alerts };
}

for (const command of ["update_project_timeline", "update_project_info"]) {
  test(`a failed ${command} keeps the editor open and a second close retries it`, async () => {
    const { state, requestClose, alerts } = fixture();
    const writes = [];
    state.invoke = async (name) => {
      writes.push(name);
      if (name === command) throw new Error("write failed");
    };
    assert.equal(await requestClose(), false);
    assert.equal(alerts.length, 1);
    assert.match(alerts[0], /write failed/);
    assert.equal(state.saveState, command === "update_project_timeline" ? "error" : "saved");
    writes.length = 0;
    state.invoke = async (name) => { writes.push(name); };
    assert.equal(await requestClose(), true);
    assert.deepEqual(writes, [command]);
  });
}

test("closing shares an in-flight timeline write and waits for edits made during it", async () => {
  const { state, requestClose } = fixture();
  state.title = state.project.title;
  const first = Promise.withResolvers();
  const snapshots = [];
  state.invoke = async (_, { timeline }) => {
    snapshots.push(timeline);
    if (snapshots.length === 1) await first.promise;
  };
  const initial = state.timeline;
  const autosave = state.flushSave();
  const close = requestClose();
  state.timeline = { clips: [{ id: "new clip" }] };
  state.saveState = "pending";
  first.resolve();
  assert.equal(await close, true);
  await autosave;
  assert.deepEqual(snapshots, [initial, state.timeline]);
  assert.equal(state.saveState, "saved");
});

test("a pending output settings failure vetoes the close without a second alert", async () => {
  const { state, requestClose, alerts } = fixture();
  const resolution = Promise.withResolvers();
  state.resolutionSaving = resolution.promise;
  const close = requestClose();
  resolution.resolve(false);
  assert.equal(await close, false);
  assert.deepEqual(alerts, []);
});
