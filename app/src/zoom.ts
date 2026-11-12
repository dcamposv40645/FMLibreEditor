// How large the main window draws the interface: the webview's own zoom, as a browser's Ctrl + and Ctrl − give, so
// that the app reads from further away on a big monitor. It is the webview's zoom rather than CSS `zoom`, so that the
// menus and popups the app places from their button's box (see SettingsMenu.tsx, CaptainMenu.tsx) still land under it.
// The level is kept between runs. The overlay, a small window laid out to fit, keeps its own size; in a browser
// (`npm run dev`) the browser's own zoom does the same, and the app leaves it alone.

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { inTauri } from "./api";
import { isOverlay } from "./overlay";

/** The levels on offer, smallest first. */
export const ZOOM_LEVELS: readonly number[] = [0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

/** Where the chosen level is kept. */
const STORAGE_KEY = "fmlibre.zoom";

/** Whether this window can be zoomed: the main window of the app. */
export const zooms: boolean = inTauri && !isOverlay;

/** The level the user chose earlier; 100% when none is saved or it cannot be read. */
function savedZoom(): number {
  try {
    const saved = Number(window.localStorage.getItem(STORAGE_KEY));
    return ZOOM_LEVELS.includes(saved) ? saved : 1;
  } catch {
    return 1;
  }
}

let level = savedZoom();
const listeners = new Set<(level: number) => void>();

/** The level now. */
export const zoomLevel = (): number => level;

/** Calls `listener` with each new level; returns what stops it. */
export function onZoom(listener: (level: number) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Draws the window at `next`, one of `ZOOM_LEVELS`, and keeps it for the next run. */
export function setZoom(next: number): void {
  if (!zooms || !ZOOM_LEVELS.includes(next)) {
    return;
  }
  level = next;
  void getCurrentWebview()
    .setZoom(next)
    .catch(() => {});
  try {
    window.localStorage.setItem(STORAGE_KEY, String(next));
  } catch {
    // Not kept: this run still shows it.
  }
  listeners.forEach((listener) => listener(next));
}

/** One level larger (`by` 1) or smaller (-1), as far as the levels go. */
export function stepZoom(by: 1 | -1): void {
  const at = ZOOM_LEVELS.indexOf(level);
  const next = ZOOM_LEVELS[Math.min(ZOOM_LEVELS.length - 1, Math.max(0, at + by))];
  setZoom(next);
}

/**
 * Draws the window at the level kept, and lets Ctrl (⌘ on the Mac) with + or = zoom in, with − zoom out and with 0
 * go back to 100%, as browsers do. Called once, when the app starts.
 */
export function startZoom(): void {
  if (!zooms) {
    return;
  }
  if (level !== 1) {
    void getCurrentWebview()
      .setZoom(level)
      .catch(() => {});
  }
  window.addEventListener("keydown", (event) => {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) {
      return;
    }
    switch (event.key) {
      case "+":
      case "=":
        stepZoom(1);
        break;
      case "-":
      case "_":
        stepZoom(-1);
        break;
      case "0":
        setZoom(1);
        break;
      default:
        return;
    }
    event.preventDefault();
  });
}

/** The keys that do `key` (+, − or 0), as the system writes them: ⌘+ on the Mac, Ctrl++ elsewhere. */
export const zoomKeys = (key: string): string =>
  /Mac|iPhone|iPad/.test(navigator.platform) ? `⌘${key}` : `Ctrl+${key}`;

/** A level as the menu shows it: "125%". */
export const zoomPercent = (at: number): string => `${Math.round(at * 100)}%`;
