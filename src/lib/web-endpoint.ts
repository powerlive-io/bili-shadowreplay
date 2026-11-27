// This module is also used before the app starts. Importing it must not open
// the event socket or contact an API.
declare global {
  interface Window {
    __TAURI_INTERNALS__?: any;
  }
}

const storageKey = "endpoint";
export const TAURI_ENV = typeof window.__TAURI_INTERNALS__ !== "undefined";

export function normalizeEndpoint(value: string): string {
  const input = value.trim();
  if (!/^https?:\/\//i.test(input)) return "";
  try {
    const url = new URL(input);
    if (url.username || url.password || input.includes("?") || input.includes("#")) return "";
    return url.href.replace(/\/+$/, "");
  } catch {
    return "";
  }
}

export const ENDPOINT = TAURI_ENV ? "" : normalizeEndpoint(localStorage.getItem(storageKey) || "");

export async function connectEndpoint(value: string) {
  const endpoint = normalizeEndpoint(value);
  if (!endpoint) throw new Error("请输入完整的 HTTP 或 HTTPS 地址，不要包含账号、密码、查询参数或片段。");

  // Check a small read-only response rather than fetching the user's config
  // (which can contain credentials) or accepting a proxy's HTML landing page.
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch(`${endpoint}/api/get_account_count`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
      signal: controller.signal,
    }).catch(() => {
      throw new Error("无法连接服务器，请检查地址、网络以及浏览器是否允许访问该地址。");
    });
    if (!response.ok) throw new Error(`连接失败（HTTP ${response.status}）。请填写服务器地址，不要附加 /api。`);
    const result = await response.json().catch(() => null);
    if (result?.code !== 0 || !Number.isInteger(result.data)) {
      throw new Error("该地址未返回 BSR API 响应，请确认服务和端口。");
    }
  } finally {
    clearTimeout(timer);
  }
  localStorage.setItem(storageKey, endpoint);
}

export function forgetEndpoint() {
  localStorage.removeItem(storageKey);
}
