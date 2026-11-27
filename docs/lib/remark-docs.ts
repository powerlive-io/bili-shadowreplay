import type { Root } from 'mdast';
import { visit } from 'unist-util-visit';
import { siteUrl } from './layout.shared';

/** Keep repository Markdown links usable on the exported website. */
export function remarkDocs() {
  return (tree: Root) => {
    visit(tree, 'blockquote', (node) => {
      const first = node.children[0];
      if (first?.type !== 'paragraph' || first.children[0]?.type !== 'text') return;
      const text = first.children[0];
      const match = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\](?:\n|$)/.exec(text.value);
      if (!match) return;
      text.value = text.value.slice(match[0].length);
      if (!text.value) first.children.shift();
      if (!first.children.length) node.children.shift();
      node.data = { ...node.data, hProperties: { 'data-callout': match[1] } };
    });
    // A standalone figure link remains a normal link in Markdown readers.
    // In the site it becomes a figure, with the SVG present even without JS.
    visit(tree, 'paragraph', (node) => {
      const [link] = node.children;
      if (node.children.length !== 1 || link?.type !== 'link') return;
      const pathname = link.url.startsWith(`${siteUrl}/`) ? link.url.slice(siteUrl.length) : link.url;
      const match = /^\/diagrams\/([a-z0-9-]+)\.html$/.exec(pathname);
      if (match) node.data = { ...node.data, hName: 'figure', hProperties: { 'data-bsr-diagram': match[1] } };
    });
    visit(tree, 'link', (node) => {
      if (!/^(?:[a-z]+:|\/\/)/i.test(node.url)) {
        node.url = node.url.replace(/\.md(?=#|$)/, '');
      }
    });
  };
}
