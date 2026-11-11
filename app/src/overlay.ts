// The overlay: a second window of the app, compact and always on top, which ⌥⇧E (Alt+Shift+E) shows over the game and
// hides again (see src-tauri/src/overlay.rs). It is this same interface, which knows it is the overlay by its window's
// label (in a browser, by `?overlay` in the address), and lays the profile out over the whole window. Both windows share
// the connection; each tells the other when it has read the game again or written to it, and the other reads its lists
// again then.

import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef } from "react";
import { call, inTauri } from "./api";

/** Whether this window is the overlay. */
export const isOverlay: boolean = inTauri
  ? getCurrentWindow().label === "overlay"
  : new URLSearchParams(window.location.search).has("overlay");

/** Shows the overlay over the game, or hides it when it is in front. */
export const toggleOverlay = () => call<void>("toggle_overlay");

/** What a window tells the other: it read the game again, or it wrote to it (or undid a change). */
type News = "reloaded" | "edited";

const EVENT = "fmlibre-news";

/** Tells the other window `news`; nothing in a browser, which has one window. */
export function tellOtherWindow(news: News): void {
  if (!inTauri) {
    return;
  }
  void emitTo(isOverlay ? "main" : "overlay", EVENT, news).catch(() => {
    // The overlay has not been opened: nobody to tell.
  });
}

/** Calls `onNews` with what the other window tells this one. */
export function useOtherWindow(onNews: (news: News) => void): void {
  const latest = useRef(onNews);
  latest.current = onNews;
  useEffect(() => {
    if (!inTauri) {
      return;
    }
    const unlisten = listen<News>(EVENT, (event) => latest.current(event.payload));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
}
