/** Escape PageView's transform so fixed dialogs cover the whole window. */
export function mountToViewport(node: HTMLElement) {
  node.ownerDocument.body.appendChild(node);
  return { destroy() { node.remove(); } };
}
