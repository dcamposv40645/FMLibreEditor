// Following the game's screen: with the setting on, the Rust side says what FM's screen shows each time it changes to
// a player, a member of staff, a team or a club (`game-viewing`, see src-tauri/src/follow.rs), and the window opens
// its profile: the player's or the staff member's, the club's, or the club's squad on that team. Both windows do, the
// main one and the overlay. Not while a dialog is up or a profile is being edited (see refreshHold.ts): what the
// screen shows then is opened once the last of them lets go. The setting is on by default; each computer keeps it for
// itself, and both windows share it.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { call, inTauri } from "./api";
import { onRefreshReleased, refreshHeld } from "./refreshHold";
import type { ClubSummary, PlayerSummary, StaffSummary } from "./types";

/** What the game's screen shows, with the row its profile opens with. */
export type Viewing =
  | { kind: "player"; player: PlayerSummary }
  | { kind: "staff"; staff: StaffSummary }
  /** A team of the club, by its place in the club's list of teams: 0 is the first team. */
  | { kind: "team"; club: ClubSummary; team: number }
  | { kind: "club"; club: ClubSummary };

/** What the game's screen showed, for the lists to open: `seq` tells one time from the next. */
export interface Followed {
  viewing: Viewing;
  seq: number;
}

const SETTING = "fmlibre.followView";

function storedSetting(): boolean {
  try {
    return localStorage.getItem(SETTING) !== "off";
  } catch {
    return true;
  }
}

/** The setting, and what changes it. The Rust side is told, and so is the other window, through the storage. */
export function useFollowViewSetting(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(storedSetting);
  useEffect(() => {
    if (inTauri) {
      void call<void>("set_follow_view", { on }).catch(() => {
        // An older Rust side: nothing to follow.
      });
    }
  }, [on]);
  useEffect(() => {
    const changed = (event: StorageEvent) => {
      if (event.key === SETTING) {
        setOn(storedSetting());
      }
    };
    window.addEventListener("storage", changed);
    return () => window.removeEventListener("storage", changed);
  }, []);
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

/** While `enabled` and connected, calls `onView` with what the game's screen shows each time it changes. */
export function useFollowView(enabled: boolean, connected: boolean, onView: (viewing: Viewing) => void): void {
  const latest = useRef(onView);
  latest.current = onView;

  useEffect(() => {
    if (!enabled || !connected || !inTauri) {
      return;
    }
    // What the screen showed while something held the profile open, for when it lets go.
    let pending: Viewing | null = null;
    const show = () => {
      if (pending !== null && !refreshHeld()) {
        const viewing = pending;
        pending = null;
        latest.current(viewing);
      }
    };
    const stopReleased = onRefreshReleased(show);
    const unlisten = listen<Viewing>("game-viewing", (event) => {
      pending = event.payload;
      show();
    });
    return () => {
      stopReleased();
      void unlisten.then((stop) => stop());
    };
  }, [enabled, connected]);
}
