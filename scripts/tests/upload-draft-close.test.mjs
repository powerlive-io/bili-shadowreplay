import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = await readFile(new URL("../../src/lib/components/upload/UploadEditor.svelte", import.meta.url), "utf8");
const parsed = ts.createSourceFile("editor.ts", source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const names = ["scheduleSave", "flushSave", "writeForm", "close", "discard", "adopt"];
const handlers = parsed.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
const code = ts.transpileModule(handlers.map(node => node.getText(parsed)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText;

function editor(invoke) {
  const state = vm.createContext({
    dirty: false, editable: true, closing: false, saving: null, saveTimer: null, saveError: "",
    setTimeout: () => 1, clearTimeout() {}, $state: { snapshot: structuredClone },
    base: { uid: "", profile: "{}" }, uid: "1", profile: { title: "edited" }, upload: { id: 1 },
    onClose: () => state.closes++, closes: 0, saveRoomTemplate() {}, invoke,
  });
  vm.runInContext(code, state);
  return state;
}

test("failed close keeps the input across refresh and a second close retries the draft", async () => {
  const f = editor(async () => { throw Error("offline"); });
  f.scheduleSave();
  await f.close();
  assert.equal(f.closes, 0);
  assert.match(f.saveError, /offline/);
  f.upload.profile = '{"title":"remote"}';
  f.adopt();
  assert.equal(f.profile.title, "edited");
  f.invoke = async (_, args) => {
    assert.equal(args.baseProfile, "{}");
    return JSON.stringify(args.profile);
  };
  await f.close();
  assert.equal(f.closes, 1);
  assert.equal(f.base.profile, '{"title":"edited"}');
});

test("close waits for autosave and drains newer input, without a duplicate close", async () => {
  const first = Promise.withResolvers(), second = Promise.withResolvers();
  const titles = [];
  const f = editor(async (_, args) => {
    titles.push(args.profile.title);
    return (titles.length === 1 ? first : second).promise;
  });
  f.scheduleSave();
  const auto = f.flushSave();
  const close = f.close();
  await f.close();
  f.profile.title = "latest";
  f.scheduleSave();
  first.resolve('{"title":"edited"}');
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.closes, 0);
  assert.deepEqual(titles, ["edited", "latest"]);
  second.resolve('{"title":"latest"}');
  await Promise.all([auto, close]);
  assert.equal(f.closes, 1);
});
