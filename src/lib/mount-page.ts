import { mount, type Component } from "svelte";
import Connect from "../page/Connect.svelte";
import { ENDPOINT, TAURI_ENV } from "./web-endpoint";

// The application pages import the invoker, which connects the event socket
// immediately. Don't even import a page until its transport is configured.
export async function mountPage(load: () => Promise<{ default: Component }>) {
  const Page = TAURI_ENV || ENDPOINT ? (await load()).default : Connect;
  return mount(Page, { target: document.getElementById("app")! });
}
