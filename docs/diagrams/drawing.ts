// Coordinates are drawing units, not CSS pixels. The viewBox fits the artwork
// to the reader's column; titles and captions use the page's normal text size.
type Point = [number, number];
type Line = 'return' | 'event';
type Box = 'node' | 'focus' | 'store' | 'terminal';
type Wire = (points: Point[], kind?: Line) => string;

export interface Diagram {
  id: string;
  /** First and last drawing rows, including padding around the actual content. */
  rows: [top: number, bottom: number];
  title: string;
  description: string;
  draw: (wire: Wire) => string[];
}

export function escape(text: string) {
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
}

export function text(x: number, y: number, content: string, cls: string, anchor = 'start') {
  return `<text x="${x}" y="${y}" class="${cls}" text-anchor="${anchor}">${escape(content)}</text>`;
}

/** Draw regions before wires. A boundary encloses a named scope, not the whole canvas by default. */
export function region(x: number, y: number, w: number, h: number, name: string, kind: 'panel' | 'boundary' = 'panel') {
  const paint = kind === 'boundary'
    ? 'fill="none" stroke="var(--stroke)" stroke-width="1.6" stroke-dasharray="6 5"'
    : 'fill="var(--panel)"';
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="12" ${paint}/>`
    + text(x + 16, y + 28, name, 'fig-heading');
}

/** Boxes are 64 units high; technical identifiers keep their exact spelling. */
export function box(x: number, y: number, w: number, name: string, sub: string, kind: Box = 'node') {
  const isCode = /^[\x20-\x7e·]+$/.test(sub);
  const fill = kind === 'focus' ? 'clay' : kind === 'store' ? 'storage' : 'surface';
  return `<g class="fig-${kind}"><rect x="${x}" y="${y}" width="${w}" height="64" rx="${kind === 'terminal' ? 20 : 10}"`
    + ` fill="var(--${fill})" stroke="var(--${kind === 'focus' ? 'clay-edge' : 'stroke'})" stroke-width="1.6"/>`
    + text(x + w / 2, y + 28, name, 'fig-name', 'middle')
    + text(x + w / 2, y + 48, sub, isCode ? 'fig-code' : 'fig-detail', 'middle') + '</g>';
}

/** Decision dimensions include the tips. Only its text, never a wire, is diagonal-bounded. */
export function diamond(x: number, y: number, w: number, h: number, name: string) {
  return `<polygon points="${x},${y-h/2} ${x+w/2},${y} ${x},${y+h/2} ${x-w/2},${y}" fill="var(--surface)" stroke="var(--stroke)" stroke-width="1.6"/>`
    + text(x, y + 4, name, 'fig-name', 'middle');
}

/** Top edge of an opaque 16px label. Leave 8px between its mask and its wire. */
export function label(cx: number, top: number, value: string, on: 'paper' | 'panel' | 'clay' = 'paper') {
  const width = 8 + 4 * Math.ceil([...value].reduce((sum, ch) => sum + (ch.charCodeAt(0) > 255 ? 13 : 8), 0) / 4);
  const markup = `<rect x="${cx - width / 2}" y="${top}" width="${width}" height="16" rx="2" fill="var(--${on})"/>`
    + text(cx, top + 12, value, 'fig-detail', 'middle');
  return on === 'clay' ? `<g class="fig-focus">${markup}</g>` : markup;
}

function route(points: Point[]) {
  let d = `M ${points[0].join(' ')}`;
  for (let i = 1; i < points.length; i++) {
    const [x, y] = points[i], [px, py] = points[i - 1];
    if (x !== px && y !== py) throw new Error(`Non-orthogonal diagram edge at ${x},${y}`);
    const next = points[i + 1];
    const axis = y === py ? 'H' : 'V';
    if (!next) return `${d} ${axis} ${axis === 'H' ? x : y}`;
    const [nx, ny] = next;
    const r = 8;
    d += ` ${axis} ${axis === 'H' ? x - r * Math.sign(x - px) : y - r * Math.sign(y - py)}`;
    d += ` Q ${x} ${y} ${x + r * Math.sign(nx - x)} ${y + r * Math.sign(ny - y)}`;
  }
  return d;
}

function connector(id: string, points: Point[], kind?: Line) {
  const dash = kind ? ' stroke-dasharray="5 4"' : '';
  return `<path d="${route(points)}" fill="none" stroke="var(--stroke)" stroke-width="1.8"${dash} marker-end="url(#${id}-${kind === 'return' ? 'reply' : 'arrow'})"/>`;
}

/** A focal message highlights a specific handoff (e.g. a commit), rather than the actor's entire lifetime. */
export function sequence(wire: Wire, actors: [string, string, Box?][], messages: [number, number, string, (Line | 'focus')?][]) {
  const xs = actors.map((_, i) => 132 + i * (696 / (actors.length - 1)));
  const spacing = messages.length > 9 ? 28 : 32;
  const lastY = 252 + (messages.length - 1) * spacing;
  const parts = xs.map(x => `<line x1="${x}" x2="${x}" y1="216" y2="${lastY + 20}" stroke="var(--stroke)" stroke-dasharray="3 5"/>`);
  messages.forEach(([from, to, name, kind], i) => {
    const y = 252 + i * spacing;
    const neighbour = from + Math.sign(to - from);
    parts.push(wire([[xs[from], y], [xs[to], y]], kind === 'focus' ? undefined : kind),
      label((xs[from] + xs[neighbour]) / 2, y - 24, name, kind === 'focus' ? 'clay' : 'paper'));
  });
  return parts.concat(actors.map(([name, sub, kind], i) => box(xs[i] - 92, 152, 184, name, sub, kind)));
}

function renderSvg(fig: Diagram) {
  const { id, rows: [top, bottom] } = fig;
  const wire: Wire = (points, kind) => connector(id, points, kind);
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 ${top} 960 ${bottom - top}" role="img" aria-labelledby="${id}-title ${id}-desc">`,
    `<title id="${id}-title">${escape(fig.title)}</title>`,
    `<desc id="${id}-desc">${escape(fig.description)}</desc>`,
    `<defs>
  <marker id="${id}-arrow" markerUnits="userSpaceOnUse" markerWidth="11" markerHeight="14" refX="8" refY="7" orient="auto">
    <path d="M 2 1 L 8 7 L 2 13" fill="none" stroke="var(--stroke)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
  </marker>
  <marker id="${id}-reply" markerWidth="6" markerHeight="6" refX="6" refY="3" orient="auto">
    <path d="M 0 0 L 6 3 L 0 6 Z" fill="var(--stroke)"/>
  </marker>
</defs>`,
    `<rect x="0" y="${top}" width="960" height="${bottom - top}" rx="14" fill="var(--paper)"/>`,
    ...fig.draw(wire), '</svg>',
  ].join('\n');
}

/** Inner markup of the figure, shared by the Markdown embed and offline HTML. */
export function renderFigure(fig: Diagram, location: 'article' | 'standalone') {
  const title = location === 'article' ? 'strong' : 'h1';
  return `<figcaption><${title} class="fig-title">${escape(fig.title)}</${title}>
<span class="fig-summary">${escape(fig.description)}</span></figcaption>
${renderSvg(fig)}`;
}
