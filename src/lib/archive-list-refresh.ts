import { createStateRefresh, watchStateChanges } from "./state-refresh";
import type { ArchiveViewMode } from "./stores/archive-view";

/** Scope each refresh to a visible view. Leaving a calendar cannot start a
 * second timing page or let a slow response overwrite the new list. */
export function createArchiveListRefresh(
  load: (view: ArchiveViewMode, active: () => boolean) => Promise<void>,
  events: string[],
  delayMs = 200,
) {
  let current: ReturnType<typeof createStateRefresh> | undefined;
  return {
    watch(view: ArchiveViewMode | null) {
      if (view === null) return;
      const active = (): boolean => current === refresh && document.visibilityState !== "hidden";
      const refresh = createStateRefresh(async () => {
        if (active()) await load(view, active);
      }, delayMs);
      current = refresh;
      const stop = watchStateChanges(refresh, events);
      return () => { current = undefined; stop(); };
    },
    async run() { await current?.run(); },
  };
}
