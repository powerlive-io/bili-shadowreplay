import "./styles.css";
import { mountPage } from "./lib/mount-page";

void mountPage(() => import("./AppClip.svelte"));
