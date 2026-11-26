import type { CoverElement, CoverProject, Ratio } from "../../cover-elements";

/** Snapshot each used sticker once, including imported or built-in artwork.
 * The result survives sticker-library deletion and a different server address. */
export async function packProject(
  state: Omit<CoverProject, "version" | "stickers">,
  previous: Record<string, string>,
  rasterize: (url: string) => Promise<string>,
): Promise<CoverProject> {
  const keys = new Map<string, string>();
  const stickers: Record<string, string> = {};
  const covers: Record<Ratio, CoverElement[]> = { wide: [], home: [] };
  for (const ratio of ["wide", "home"] as const) {
    for (const element of state.covers[ratio]) {
      if (element.kind === "text") {
        covers[ratio].push({ ...element });
        continue;
      }
      let key = keys.get(element.src);
      if (!key) {
        key = `asset:${keys.size}`;
        keys.set(element.src, key);
        stickers[key] = Object.prototype.hasOwnProperty.call(previous, element.src)
          ? previous[element.src] : await rasterize(element.src);
      }
      covers[ratio].push({ ...element, src: key });
    }
  }
  return { ...state, version: 1, stickers, covers };
}
