import assert from "node:assert/strict";
import { test } from "node:test";
import { loadLibModule } from "./ts-module-fixture.mjs";

async function fixture() {
  const calls = [];
  const invoker = { invoke: async (command, args) => {
    calls.push({ command, args });
    return "completed";
  } };
  const tools = await loadLibModule("agent/tools", [], {
    "../invoker": invoker,
    "../interface": await loadLibModule("interface", [], { "../lib/invoker": invoker }),
    "./messages": await loadLibModule("agent/messages"),
  });
  return { invoke: tools.invokeToolByName, calls };
}

test("confirmed subtitle generation sends a new task ID with the material ID", async () => {
  const f = await fixture();
  for (let i = 0; i < 2; i++) {
    assert.equal(await f.invoke("generate_video_subtitle", { id: 42 }), "completed");
  }
  assert.equal(f.calls.length, 2);
  for (const { command, args } of f.calls) {
    assert.equal(command, "generate_video_subtitle");
    assert.equal(args.id, 42);
    assert.match(args.eventId, /\S/);
  }
  assert.notEqual(f.calls[0].args.eventId, f.calls[1].args.eventId);
});

test("stale arbitrary-command confirmations cannot invoke the backend", async () => {
  const f = await fixture();
  await assert.rejects(
    f.invoke("generic_ffmpeg_command", { args: ["-version"] }),
    /not available for frontend confirmation/,
  );
  assert.equal(f.calls.length, 0);
});

test("structured media confirmations retain their operation parameters", async () => {
  const f = await fixture();
  assert.equal(await f.invoke("extract_video_audio", { videoId: 42 }), "completed");
  assert.equal(await f.invoke("merge_videos", {
    videoIds: [42, 43], outputTitle: "Joined", outputNote: "Notes", transition: "fade",
  }), "completed");
  assert.deepEqual(JSON.parse(JSON.stringify(f.calls)), [
    { command: "extract_video_audio", args: { videoId: 42 } },
    { command: "merge_videos", args: {
      videoIds: [42, 43], outputTitle: "Joined", outputNote: "Notes", transition: "fade",
    } },
  ]);
});

test("argument keys stay data and canonical names win throughout nested parameters", async () => {
  const source = `{"clipRangeParams": {
    "__proto__": {"title": "inherited"}, "Title": "explicit", "Constructor": "metadata",
    "roomId": "alias", "room_id": "canonical",
    "ranges": [{"__proto__": {"start": 99}, "Start": 1, "End": 2}, {"Start": 4, "start": 3, "End": 5}]
  }}`;
  for (const args of [source, JSON.parse(source)]) {
    const f = await fixture();
    await f.invoke("clip_range", args);
    const params = f.calls[0].args.params;
    assert.deepEqual(JSON.parse(JSON.stringify(params)), {
      ["__proto__"]: { title: "inherited" },
      title: "explicit",
      constructor: "metadata",
      room_id: "canonical",
      ranges: [{ ["__proto__"]: { start: 99 }, start: 1, end: 2 }, { start: 3, end: 5 }],
    });
    assert.equal(Object.getPrototypeOf(params.ranges[0]), Object.getPrototypeOf(f.calls[0].args));
  }
});


test("subtitle and merge confirmations pass selected resolution to the backend", async () => {
  const f = await fixture();
  const settings = { codec: "hevc", priority: "speed", bitrate_kbps: 10000, resolution: "original" };
  await f.invoke("encode_video_subtitle", { id: 42, srt_style: "", render_settings: settings });
  await f.invoke("merge_videos", {
    video_ids: [42, 43], output_title: "Portrait", output_note: "", render_settings: settings,
  });
  for (const call of f.calls) assert.deepEqual(JSON.parse(JSON.stringify(call.args.renderSettings)), settings);
});
