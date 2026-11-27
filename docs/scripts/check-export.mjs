import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { parse, serializeOuter } from 'parse5';
import { staticClient } from 'fumadocs-core/search/client/orama-static';

const root = path.resolve('out');
const origin = 'https://bsr.powerlive.io';
const files = readdirSync(root, { recursive: true }).filter((file) => file.endsWith('.html'));
const failures = [];
const pages = new Map();
let links = 0;
let fragments = 0;
function walk(node, visit) {
  visit(node);
  for (const child of node.childNodes ?? []) walk(child, visit);
}
for (const file of files) {
  const html = readFileSync(path.join(root, file), 'utf8');
  const tree = parse(html);
  const ids = new Set();
  walk(tree, (node) => {
    for (const attr of node.attrs ?? []) {
      if (attr.name === 'id' || (node.tagName === 'a' && attr.name === 'name')) ids.add(attr.value);
    }
  });
  pages.set(path.join(root, file), { tree, ids });
  assert(!html.includes('bsr.xinrea.cn'), `Old domain in ${file}`);
}
for (const file of files) {
  const url = new URL(file === 'index.html' ? '/' : `/${file.replace(/\.html$/, '')}`, origin);
  walk(pages.get(path.join(root, file)).tree, (node) => {
    for (const { name, value } of node.attrs ?? []) {
      if (!['href', 'src'].includes(name) || !value || /^(data:|mailto:|tel:)/.test(value)) continue;
      const target = new URL(value, url);
      if (target.origin !== origin) continue;
      const pathname = decodeURIComponent(target.pathname);
      const base = path.join(root, pathname);
      const resolved = [base, `${base}.html`, path.join(base, 'index.html')].find((candidate) => existsSync(candidate) && statSync(candidate).isFile());
      if (!resolved) failures.push(`${file}: ${value}`);
      const targetPage = pages.get(resolved);
      if (name === 'href' && target.hash && targetPage) {
        // Text fragments are browser directives, not element IDs.
        const fragment = decodeURIComponent(target.hash.slice(1).split(':~:text=')[0]);
        if (fragment && !targetPage.ids.has(fragment)) failures.push(`${file}: missing anchor ${value}`);
        fragments++;
      }
      links++;
    }
  });
}
assert.deepEqual(failures, [], 'Broken internal links, anchors or missing assets');
const content = ['getting-started', 'usage', 'development'].flatMap((dir) => readdirSync(dir, { recursive: true }).filter((f) => f.endsWith('.md')).map((f) => `${dir}/${f}`));
for (const file of content) assert(existsSync(path.join(root, file.replace(/\.md$/, '.html'))), `Missing page: ${file}`);
const testingUrl = '/development/testing/overview';
const legacyTesting = pages.get(path.join(root, 'TESTING.html'));
assert(legacyTesting, 'Legacy testing URL remains available');
let testingRedirect;
walk(legacyTesting.tree, (node) => {
  const attrs = Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value]));
  if (node.tagName === 'meta' && attrs['http-equiv'] === 'refresh') testingRedirect = attrs.content;
});
assert.equal(testingRedirect, `0; url=${testingUrl}`, 'Legacy testing URL redirects to the new guide');
const rootNavigation = JSON.parse(readFileSync('meta.json', 'utf8'));
const developmentNavigation = JSON.parse(readFileSync('development/meta.json', 'utf8'));
assert(!rootNavigation.pages.includes('TESTING'), 'Testing is no longer a top-level navigation item');
assert(developmentNavigation.pages.includes('testing'), 'Testing belongs under development');
assert.equal(readFileSync(path.join(root, 'CNAME'), 'utf8').trim(), 'bsr.powerlive.io');
assert(existsSync(path.join(root, '.nojekyll')));
assert.match(readFileSync(path.join(root, 'index.html'), 'utf8'), /切片编辑/, 'Homepage is prerendered');
assert.match(readFileSync(path.join(root, 'usage/features/room.html'), 'utf8'), /<video[^>]+src="\/videos\/deeplinking.mp4"/, 'Inline videos survive Markdown compilation');

// The article and its offline download must show the same accessible SVG,
// without waiting for hydration. Every exported figure belongs to an article.
const standalone = files.filter(file => file.startsWith(`diagrams${path.sep}`));
const embedded = new Set();
const attr = (node, name) => node.attrs?.find(a => a.name === name)?.value;
function elements(tree, tag) {
  const matches = [];
  walk(tree, node => { if (node.tagName === tag) matches.push(node); });
  return matches;
}
for (const file of content) {
  const page = pages.get(path.join(root, file.replace(/\.md$/, '.html')));
  const ids = new Set();
  for (const figure of elements(page.tree, 'figure')) {
    const id = attr(figure, 'data-bsr-diagram');
    if (!id) continue;
    assert(!ids.has(id), `${file}: duplicate figure IDs for ${id}`);
    ids.add(id);
    const target = path.join('diagrams', `${id}.html`);
    const offline = pages.get(path.join(root, target));
    assert(offline, `Missing offline diagram: ${target}`);
    assert(!elements(figure, 'a').some(link => attr(link, 'href') === `/diagrams/${id}.html`),
      `${file}: embedded ${id} should not link to its standalone copy`);
    const [svg] = elements(figure, 'svg');
    assert(svg && attr(svg, 'role') === 'img', `${file}: ${id} must contain a static image`);
    assert.equal(attr(svg, 'aria-labelledby'), `${id}-title ${id}-desc`);
    for (const tag of ['title', 'desc']) {
      const [child] = elements(svg, tag);
      assert(child && attr(child, 'id') === `${id}-${tag}` && child.childNodes[0]?.value?.trim(), `${id}: missing accessible ${tag}`);
    }
    const [copy] = elements(offline.tree, 'svg');
    assert(copy && serializeOuter(copy) === serializeOuter(svg), `${id}: standalone and article figures differ`);
    // Keep the human-readable title/summary in normal document flow: unlike
    // SVG text, these wrap at the column width instead of shrinking with it.
    for (const tree of [figure, offline.tree]) {
      const [caption] = elements(tree, 'figcaption');
      assert(caption, `${id}: missing figure caption`);
      for (const [tag, cls] of [['title', 'fig-title'], ['desc', 'fig-summary']]) {
        const expected = elements(svg, tag)[0].childNodes[0].value;
        const entry = caption.childNodes.find(node => attr(node, 'class') === cls);
        assert.equal(entry?.childNodes[0]?.value, expected, `${id}: caption differs from accessible ${tag}`);
      }
    }
    assert.equal(elements(offline.tree, 'script').length, 0, `${id}: standalone diagram must not need a runtime`);
    embedded.add(target);
  }
}
assert(embedded.has(path.join('diagrams', 'publishing-workflow.html')), 'Workflow includes its static figure');
assert.deepEqual(embedded, new Set(standalone), 'Unreferenced or missing standalone diagrams');

const index = readFileSync(path.join(root, 'api/search'), 'utf8');
assert(!index.includes('/TESTING'), 'Legacy testing content is absent from search');
const client = staticClient({ from: `data:application/json;base64,${Buffer.from(index).toString('base64')}` });
for (const [query, expected] of [['字幕', '/usage/features/subtitle'], ['Docker', '/getting-started/installation/docker'], ['录制', '/development/backend/recorder-manager'], ['音频回归', '/development/testing/audio'], ['测试概览', testingUrl]]) {
  const results = await client.search(query);
  assert(results.some((result) => result.url?.startsWith(expected)), `Search failed: ${query}`);
}
console.log(`Verified ${content.length} documentation pages, ${embedded.size} inline/offline figures, legacy testing redirect, testing navigation, ${links} internal links/assets (${fragments} fragments), homepage, videos and Chinese/English static search.`);
