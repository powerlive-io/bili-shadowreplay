import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import ts from "typescript";

const context = vm.createContext({});
const helperSource = await readFile(new URL("../../src/lib/task-progress.ts", import.meta.url), "utf8");
const helpers = new vm.SourceTextModule(ts.transpileModule(helperSource, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText, { context });
await helpers.link(() => { throw new Error("Unexpected progress dependency"); });
await helpers.evaluate();
const source = await readFile(new URL("../../src/lib/components/TaskProgress.svelte", import.meta.url), "utf8");
const component = new vm.SourceTextModule(compile(source, { generate: "server" }).js.code, { context });
await component.link(async (name) => {
  if (name === "../task-progress") return helpers;
  const exports = await import(name);
  return new vm.SyntheticModule(Object.keys(exports), function () {
    for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
  }, { context });
});
await component.evaluate();

test("preparation messages expose file and byte counts without a false percent", () => {
  const content = "准备录播 1/2：65/2275 文件，已读取 210.6 MiB";
  const { body } = render(component.namespace.default, {
    props: { update: { id: "task", content } },
  });
  assert.match(body, /<span class="[^"]*break-words[^"]*">准备录播 1\/2：65\/2275 文件，已读取 210\.6 MiB<\/span>/);
  assert.match(body, /indeterminate/);
  assert.doesNotMatch(body, /aria-valuenow|w-\[8\.5rem\]/);
});

test("preparation survives reopening from the saved task message", () => {
  const { body } = render(component.namespace.default, {
    props: { update: null, placeholder: "准备录播 1/1：连接存储" },
  });
  assert.match(body, /准备录播 1\/1：连接存储/);
  assert.doesNotMatch(body, /等待开始|aria-valuenow/);
});

test("measured media stages keep their percentage and remaining time", () => {
  const { body } = render(component.namespace.default, {
    props: { update: { id: "task", content: "压制弹幕 50%", progress: {
      stage: "压制弹幕", processed: 30, total: 60, speed: 2,
    } } },
  });
  assert.match(body, /aria-valuenow="50"/);
  assert.match(body, /剩余约 15 秒/);
  assert.match(body, /w-\[8\.5rem\]/);
  assert.doesNotMatch(body, /class="indeterminate/);
});
