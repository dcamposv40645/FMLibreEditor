// What keeps the app from reading the game again by itself (see the auto-refresh in App.tsx): a dialog that is up, or a
// profile being edited. Reading the game again then would close the one or lose what was typed into the other, so the
// refresh waits until the last of them lets go, and is told when it does.

import { useEffect } from "react";

const holds = new Set<object>();
const released = new Set<() => void>();

/** While `on`, the app does not read the game again by itself. */
export function useRefreshHold(on: boolean): void {
  useEffect(() => {
    if (!on) {
      return;
    }
    const hold = {};
    holds.add(hold);
    return () => {
      holds.delete(hold);
      if (holds.size === 0) {
        released.forEach((listener) => listener());
      }
    };
  }, [on]);
}

/** Whether something holds the refresh now. */
export const refreshHeld = (): boolean => holds.size > 0;

/** Calls `listener` each time the last hold lets go; returns what stops that. */
export function onRefreshReleased(listener: () => void): () => void {
  released.add(listener);
  return () => {
    released.delete(listener);
  };
}
