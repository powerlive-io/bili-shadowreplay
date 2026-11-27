import "./styles.css";
import { mountPage } from "./lib/mount-page";
import { initTheme } from "./lib/stores/theme";

initTheme();
void mountPage(() => import("./App.svelte"));
