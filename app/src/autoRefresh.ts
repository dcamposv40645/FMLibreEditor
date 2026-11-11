// Reading the game again by itself, as Reload data does, once the game has moved on: the Rust side looks at the in-game
// date every two seconds and says when it changes (`game-moved`, see `game_moved` in src-tauri/src/lib.rs). The app
// waits until the date has stopped changing for a moment, so that days played in a row are read once, and until no
// dialog is up and no profile is being edited (see refreshHold.ts). Whether it does is a setting, on by default, which
// each computer keeps for itself.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { inTauri } from "./api";
import { onRefreshReleased, refreshHeld } from "./refreshHold";

/** How long the date has to stay the same before the game is read again. */
const SETTLE_MS = 3000;

/** How soon to look again when the app was busy with something else. */
const RETRY_MS = 1000;

const SETTING = "fmlibre.autoRefresh";

/** Whether the app reads the game again by itself, as this computer keeps it; on when it keeps nothing. */
function storedSetting(): boolean {
  try {
    return localStorage.getItem(SETTING) !== "off";
  } catch {
    return true;
  }
}

/** The setting, and what changes it. */
export function useAutoRefreshSetting(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(storedSetting);
  const set = (value: boolean) => {
    setOn(value);
    try {
      localStorage.setItem(SETTING, value ? "on" : "off");
    } catch {
      // Not kept: it holds until the app closes.
    }
  };
  return [on, set];
}

/**
 * While `enabled` and connected, reads the game again (`reload`) once it has moved on, when nothing holds the refresh
 * and the app is not `busy`.
 */
export function useAutoRefresh(enabled: boolean, connected: boolean, busy: boolean, reload: () => Promise<void>): void {
  const latest = useRef({ busy, reload });
  latest.current = { busy, reload };

  useEffect(() => {
    if (!enabled || !connected || !inTauri) {
      return;
    }
    let pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const later = (ms: number) => {
      clearTimeout(timer);
      timer = setTimeout(attempt, ms);
    };
    function attempt() {
      if (!pending || stopped) {
        return;
      }
      if (refreshHeld()) {
        // Tried again when the last hold lets go.
        return;
      }
      if (latest.current.busy) {
        later(RETRY_MS);
        return;
      }
      pending = false;
      void latest.current.reload();
    }
    const stopReleased = onRefreshReleased(() => later(RETRY_MS / 2));
    const unlisten = listen("game-moved", () => {
      pending = true;
      later(SETTLE_MS);
    });
    return () => {
      stopped = true;
      clearTimeout(timer);
      stopReleased();
      void unlisten.then((stop) => stop());
    };
  }, [enabled, connected]);
}
