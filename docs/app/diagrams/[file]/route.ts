import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { diagrams, escape, renderFigure } from '@/diagrams';

export const dynamic = 'force-static';
export const dynamicParams = false;

export function generateStaticParams() {
  return Array.from(diagrams.keys(), id => ({ file: `${id}.html` }));
}

// Serve the same drawing as the article, but without the site's JS, fonts or
// navigation. The exported file can also be opened directly from disk.
export async function GET(_: Request, { params }: { params: Promise<{ file: string }> }) {
  const { file } = await params;
  const fig = file.endsWith('.html') ? diagrams.get(file.slice(0, -5)) : undefined;
  if (!fig) return new Response(null, { status: 404 });
  const css = readFileSync(join(process.cwd(), 'diagrams/figures.css'), 'utf8');
  return new Response(`<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light dark">
  <title>${escape(fig.title)} · BiliBili ShadowReplay</title>
  <style>${css}
    body { margin: 0; }
    main { max-width: 1200px; margin: 0 auto; padding: clamp(.75rem, 2vw, 1.5rem); }
    figure { margin: 0; }
  </style>
</head>
<body><main>
  <figure class="bsr-figure">${renderFigure(fig, 'standalone')}</figure>
</main></body>
</html>`, { headers: { 'Content-Type': 'text/html; charset=utf-8' } });
}
