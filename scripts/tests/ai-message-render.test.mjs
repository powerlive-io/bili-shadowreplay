import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { JSDOM } from "jsdom";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";
import { loadLibModule, syntheticModule } from "./ts-module-fixture.mjs";

globalThis.window = new JSDOM().window;
const stub = () => {};
const dependencies = {
  "../agent/messages": await loadLibModule("agent/messages"),
  "../safe-markdown": await loadLibModule("safe-markdown", ["dompurify", "marked"]),
  "./CopyMarkdownButton.svelte": { default: stub },
  "lucide-svelte": { Bot: stub, Check: stub, X: stub, AlertTriangle: stub },
};
const source = await readFile(new URL("../../src/lib/components/AIMessage.svelte", import.meta.url), "utf8");
const component = new vm.SourceTextModule(compile(source, { generate: "server" }).js.code);
await component.link(async name => syntheticModule(dependencies[name] ?? await import(name)));
await component.evaluate();

test("chat sanitizes Markdown parts without interpreting JSON or dropping image attachments", () => {
  const content = [
    '| 列 |\n| --- |\n| **内容** |',
    { type: "text", text: '<a href="/local" onclick="">说明</a><iframe></iframe>' },
    { note: "<em>literal</em>" },
    "- [x] 截图：![预览](https://example.org/preview.png)",
    { type: "image_url", image_url: { url: "https://example.org/attachment.png" } },
  ];
  const { body } = render(component.namespace.default, {
    props: { message: { kind: "assistant", content, timestamp: "2026-01-01", toolCalls: [] }, formatTime: () => "" },
  });
  const dom = JSDOM.fragment(body);
  assert.equal(dom.querySelector("td strong").textContent, "内容");
  assert.equal(dom.querySelector("a").textContent, "说明");
  assert.equal(dom.querySelector("iframe, [onclick], a[href]"), null);
  assert.equal(dom.querySelector("pre").textContent, JSON.stringify(content[2], null, 2));
  assert.ok(dom.querySelector('li input[type="checkbox"][checked][disabled]'));
  assert.deepEqual(Array.from(dom.querySelectorAll("img"), img => img.getAttribute("src")), [
    "https://example.org/preview.png", "https://example.org/attachment.png",
  ]);
});
