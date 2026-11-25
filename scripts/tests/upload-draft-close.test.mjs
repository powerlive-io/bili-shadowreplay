import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = await readFile(new URL("../../src/lib/components/upload/UploadEditor.svelte", import.meta.url), "utf8");
const ast = ts.createSourceFile("editor.ts", source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const names = ["close", "saveDraft", "persist", "draftSource", "saveCover", "setTags", "generateCover", "post"];
const handlers = ast.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
const code = ts.transpileModule(handlers.map(node => node.getText(ast)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText;

function editor() {
  const calls = [];
  const f = vm.createContext({
    calls, editable: true, busy: false, dirty: false, saving: false, posting: false,
    covers: null, confirmClose: false, generatingCover: false, saveError: "", coverEditorKey: 0,
    base: { id: 0, source: "material", source_id: 3, file: "", updated_at: "", danmu: false },
    danmu: true, settingsChanged: true, renderSettings: { bitrate_kbps: 12000 }, originalSettings: { bitrate_kbps: 10000 },
    profile: { title: "new", tid: 27 }, fallbackTid: undefined, uid: "", blocker: "",
    upload: { source: "material" }, $state: { snapshot: structuredClone },
    onClose: () => calls.push("closed"), onChanged: async () => {}, saveRoomTemplate() {},
    adopt: row => { f.base = row; }, invoke: async (cmd, args) => { calls.push([cmd, args]); return { id: 42 }; },
    renderAutoCover: async () => ({ cover: "wide", cover43: "home" }),
    generateEventId: () => "event", announceTask() {}, openTaskCenter() {}, settleTask() {},
  });
  vm.runInContext(code, f);
  return f;
}

test("closing a clean preview does not create a row; dirty close only asks", () => {
  const f = editor();
  f.close();
  assert.deepEqual(f.calls, ["closed"]);
  f.calls.length = 0;
  f.dirty = true;
  f.close();
  assert.equal(f.confirmClose, true);
  assert.deepEqual(f.calls, []);
  assert.match(source, /onclick=\{onClose\}>不保存/);
});

test("manual and AI cover changes stay local until an explicit save", async () => {
  const f = editor();
  f.setTags(["clip"]);
  await f.saveCover({ cover: "manual", cover43: "manual43" });
  assert.deepEqual(f.calls, []);
  await f.generateCover();
  assert.deepEqual(f.calls.map(([cmd]) => cmd), ["suggest_upload_cover"]);
  assert.equal(f.calls[0][1].profile.tag, "clip");
  await f.saveDraft();
  const [cmd, { edit }] = f.calls[1];
  assert.equal(cmd, "save_upload_draft");
  assert.equal(edit.uid, "", "saving needs no posting account");
  assert.equal(edit.covers.cover, "wide");
  assert.equal(edit.danmu.value, true);
  assert.equal(edit.renderSettings.value.bitrate_kbps, 12000);
  assert.equal(f.calls[2], "closed");
});

test("save failure retains all input; retry closes only after acknowledgement", async () => {
  const f = editor();
  f.invoke = async () => { throw Error("offline"); };
  await f.saveDraft();
  assert.deepEqual(f.calls, []);
  assert.match(f.saveError, /offline/);
  assert.equal(f.profile.title, "new");
  assert.equal(f.base.id, 0);
  const ack = Promise.withResolvers();
  f.invoke = () => ack.promise;
  const retry = f.saveDraft();
  assert.deepEqual(f.calls, []);
  ack.resolve({ id: 3 });
  await retry;
  assert.deepEqual(f.calls, ["closed"]);
});

test("direct posting uses the id returned by the explicit save", async () => {
  const f = editor();
  f.uid = "1";
  await f.post();
  assert.equal(f.calls[0][0], "save_upload_draft");
  assert.equal(f.calls[1][0], "post_upload");
  assert.equal(f.calls[1][1].id, 42);
  assert.equal(f.calls[2], "closed");
});
