import createDOMPurify from "dompurify";
import { marked } from "marked";

// Links and image requests must not resolve relative to the webview's origin.
const purifier = createDOMPurify(window);
purifier.addHook("uponSanitizeAttribute", (_, attr) => {
  if ((attr.attrName === "href" || attr.attrName === "src") && !/^https?:\/\//i.test(attr.attrValue)) {
    attr.keepAttr = false;
  }
});

purifier.addHook("afterSanitizeAttributes", (node) => {
  if (node.localName === "input") {
    // Task lists need their checked state, not an interactive form control.
    if (node.getAttribute("type")?.toLowerCase() === "checkbox") node.setAttribute("disabled", "");
    else node.remove();
  } else if (node.localName === "img") {
    node.setAttribute("referrerpolicy", "no-referrer");
  }
});

export function renderSafeMarkdown(markdown: string): string {
  return purifier.sanitize(marked.parse(markdown, { async: false }), {
    ALLOWED_TAGS: [
      "h1", "h2", "h3", "h4", "h5", "h6", "p", "br", "hr", "blockquote",
      "ul", "ol", "li", "strong", "em", "del", "pre", "code", "a", "input", "img",
      "table", "thead", "tbody", "tr", "th", "td",
    ],
    ALLOWED_ATTR: ["title"],
    ADD_ATTR: (attr, tag) =>
      (tag === "a" && attr === "href") ||
      (tag === "ol" && attr === "start") ||
      (tag === "img" && (attr === "src" || attr === "alt")) ||
      (tag === "input" && (attr === "type" || attr === "checked")),
    ALLOW_ARIA_ATTR: false,
    ALLOW_DATA_ATTR: false,
  });
}
