import { SvelteMap } from "svelte/reactivity";
import type { MaterialPeaks, ProjectTimeline } from "./interface";
import type { invoke as invokeCommand } from "./invoker";

function resource<T>(description: string, fetch: (id: number) => Promise<T>) {
  const values = new SvelteMap<number, T>();
  const pending = new Map<number, Promise<void>>();

  function load(id: number) {
    if (values.has(id)) return;
    const existing = pending.get(id);
    if (existing) return existing;
    const request = fetch(id)
      .then((value) => { values.set(id, value); })
      .catch((error) => console.warn(`Failed to load ${description}:`, error))
      .finally(() => pending.delete(id));
    pending.set(id, request);
    return request;
  }

  return { values, load };
}

/** Editor-local caches: a material and a library audio file can have the same numeric ID. */
export function createProjectMaterialData(invoke: typeof invokeCommand) {
  const peaks = resource("waveform", (id) => invoke<MaterialPeaks>("get_material_peaks", { id }));
  const audioPeaks = resource("waveform", (id) => invoke<MaterialPeaks>("get_audio_peaks", { id }));
  const danmuTimes = resource("danmaku", async (id) => {
    const entries = await invoke<{ type: string; ts: number }[]>("get_material_danmu", { id });
    return entries.filter((entry) => entry.type === "danmu").map((entry) => entry.ts);
  });

  async function load(timeline: ProjectTimeline) {
    const mainIds = new Set(timeline.clips.map((clip) => clip.material_id));
    const materialIds = new Set(mainIds);
    const audioIds = new Set<number>();
    for (const track of timeline.audio_tracks) {
      for (const item of track.items) {
        if (item.kind === "material") materialIds.add(item.material_id);
        else if (item.kind === "file") audioIds.add(item.audio_id);
      }
    }
    await Promise.all([
      ...Array.from(materialIds, peaks.load),
      ...Array.from(audioIds, audioPeaks.load),
      ...Array.from(mainIds, danmuTimes.load),
    ]);
  }

  return { peaks: peaks.values, audioPeaks: audioPeaks.values, danmuTimes: danmuTimes.values, load };
}
