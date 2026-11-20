import assert from "node:assert/strict";
import test from "node:test";
import { createProjectMaterialData } from "../../src/lib/project-material-data.ts";

const timeline = {
  clips: [{ material_id: 7 }, { material_id: 7 }],
  audio_tracks: [{ items: [
    { kind: "material", material_id: 7 }, { kind: "material", material_id: 8 },
    { kind: "file", audio_id: 7 },
  ] }],
  tracks: [{ items: [{ kind: "video", material_id: 9 }] }],
};

test("overlapping edits share pending requests and reuse completed timeline data", async () => {
  const calls = [];
  const gate = Promise.withResolvers();
  const data = createProjectMaterialData(async (command, { id }) => {
    calls.push(`${command}:${id}`);
    await gate.promise;
    if (command === "get_material_danmu") return [{ type: "danmu", ts: 50 }, { type: "sc", ts: 60 }];
    return { peaks: [command === "get_audio_peaks" ? 2 : 1] };
  });

  const first = data.load(timeline);
  const second = data.load(timeline);
  const expected = ["get_audio_peaks:7", "get_material_danmu:7", "get_material_peaks:7", "get_material_peaks:8"];
  assert.deepEqual(calls.toSorted(), expected);
  gate.resolve();
  await Promise.all([first, second]);
  assert.deepEqual(data.peaks.get(7).peaks, [1]);
  assert.deepEqual(data.audioPeaks.get(7).peaks, [2]);
  assert.deepEqual(data.danmuTimes.get(7), [50]);

  await data.load(timeline);
  assert.deepEqual(calls.toSorted(), expected);
});

test("a failed resource can retry without reloading other resources for the same ID", async (t) => {
  const warn = t.mock.method(console, "warn", () => {});
  let waveformCalls = 0;
  let danmuCalls = 0;
  const data = createProjectMaterialData(async (command) => {
    if (command === "get_material_danmu") {
      danmuCalls++;
      return [];
    }
    if (++waveformCalls === 1) throw new Error("temporarily unavailable");
    return { peaks: [3] };
  });
  const oneClip = { clips: [{ material_id: 1 }], audio_tracks: [] };
  await data.load(oneClip);
  assert.equal(warn.mock.callCount(), 1);
  assert.equal(data.peaks.has(1), false);
  await data.load(oneClip);
  assert.deepEqual(data.peaks.get(1).peaks, [3]);
  assert.equal(waveformCalls, 2);
  assert.equal(danmuCalls, 1);
});
