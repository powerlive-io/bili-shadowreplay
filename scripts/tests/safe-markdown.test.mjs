import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { loadLibModule } from "./ts-module-fixture.mjs";

globalThis.window = new JSDOM().window;
const { renderSafeMarkdown } = await loadLibModule("safe-markdown", ["dompurify", "marked"]);

function render(markdown) {
  const container = window.document.createElement("div");
  container.innerHTML = renderSafeMarkdown(markdown);
  return container;
}

test("summary preserves Markdown formatting and absolute web links", () => {
  const summary = render("# 直播内容总结\n\n## 主要内容\n\n- **重点**：`<示例>`\n- [参考](https://example.org/notes)");
  assert.equal(summary.querySelector("h1").textContent, "直播内容总结");
  assert.equal(summary.querySelector("h2").textContent, "主要内容");
  assert.equal(summary.querySelector("ul li strong").textContent, "重点");
  assert.equal(summary.querySelector("code").textContent, "<示例>");
  assert.equal(summary.querySelector("li a").getAttribute("href"), "https://example.org/notes");
});

test("summary removes non-allowlisted elements and attributes from the final HTML", () => {
  const summary = render([
    '<p onclick="" style="color:red" id="custom" data-note="custom">正文</p>',
    '<script type="application/json">{}</script>',
    "<svg></svg><iframe></iframe><form></form>",
  ].join("\n"));
  assert.equal(summary.querySelector("p").textContent, "正文");
  assert.deepEqual(summary.querySelector("p").getAttributeNames(), []);
  assert.equal(summary.querySelector("script, svg, iframe, form"), null);
});

test("task lists retain checked states, but untrusted inputs cannot accept user input", () => {
  const content = render([
    "- [x] 已完成", "- [ ] 待办", "",
    '<input type="checkbox" autofocus name="custom" form="custom">',
    '<input type="text"><input type="file"><input type="image" src="https://example.org/input.png"><input>',
  ].join("\n"));
  const inputs = Array.from(content.querySelectorAll("input"));
  assert.deepEqual(inputs.map(input => [input.type, input.checked, input.disabled]), [
    ["checkbox", true, true], ["checkbox", false, true], ["checkbox", false, true],
  ]);
  assert.equal(content.querySelector("[autofocus], [name], [form], [src]"), null);
});

test("Markdown images retain their web sources and descriptions, not arbitrary request attributes", () => {
  const content = render([
    '![截图](https://example.org/frame.png "说明")',
    '<img src="http://example.org/photo.jpg" srcset="/local 2x" onerror="" referrerpolicy="unsafe-url">',
  ].join("\n"));
  const [markdown, html] = content.querySelectorAll("img");
  assert.equal(markdown.getAttribute("src"), "https://example.org/frame.png");
  assert.equal(markdown.alt, "截图");
  assert.equal(markdown.title, "说明");
  assert.equal(html.getAttribute("src"), "http://example.org/photo.jpg");
  assert.equal(html.getAttribute("referrerpolicy"), "no-referrer");
  assert.equal(content.querySelector("[srcset], [onerror]"), null);
  for (const src of ["/local", "//example.org/p.png", "file:///cover", "tauri://localhost/cover", "data:text/plain,cover", "javascript:void"]) {
    const img = render(`![封面](${src})`).querySelector("img");
    assert.equal(img.hasAttribute("src"), false, src);
    assert.equal(img.alt, "封面");
  }
});

test("link policy applies to both Markdown and inline HTML without losing their labels", () => {
  const summary = render([
    "[脚本](javascript:void) [数据](data:text/plain,note) [本地](/notes)",
    '<a href="tauri://localhost/notes">应用</a> <a href="//example.org">相对</a>',
    '<a href="HTTP://example.org" target="_blank">外部</a>',
  ].join("\n"));
  assert.deepEqual(
    Array.from(summary.querySelectorAll("a"), a => [a.textContent, a.getAttribute("href")]),
    [["脚本", null], ["数据", null], ["本地", null], ["应用", null], ["相对", null], ["外部", "HTTP://example.org"]],
  );
  assert.equal(summary.querySelector("[target]"), null);
});
