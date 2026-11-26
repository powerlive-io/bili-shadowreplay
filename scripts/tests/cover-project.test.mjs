import assert from "node:assert/strict";
import test from "node:test";
import { loadLibModule } from "./ts-module-fixture.mjs";

const { packProject } = await loadLibModule("components/cover/project");
test("saving embeds shared sticker artwork once and reopening never fetches its old URL", async () => {
  const text = { kind: "text", id: 1, content: "AI 标题" };
  const sticker = { kind: "sticker", id: 2, src: "blob:old" };
  const state = { background: "original", frames: { wide: { zoom: 1 }, home: { zoom: 2 } },
    syncCovers: false, covers: { wide: [text, sticker], home: [sticker, { ...text, x: 100 }] } };
  const loads = [];
  const saved = await packProject(state, {}, async src => { loads.push(src); return "pixels"; });
  assert.deepEqual(loads, ["blob:old"]);
  assert.equal(sticker.src, "blob:old");
  const reopened = JSON.parse(JSON.stringify(saved));
  reopened.covers.home[1].content = "改标题";
  const edited = await packProject(reopened, reopened.stickers, () => assert.fail("unexpected fetch"));
  assert.deepEqual(JSON.parse(JSON.stringify(edited)), reopened);
  assert.equal(edited.covers.wide[0].content, "AI 标题");
});
