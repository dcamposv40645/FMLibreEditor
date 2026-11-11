// What makes a dialog modal: Tab and focus stay inside it, and Escape closes it without anything behind it hearing it.

import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { useRefreshHold } from "./refreshHold";

/**
 * The dialogs that are up, the one opened last at the end. A dialog can open another (a player's profile over a club's
 * squad, then his Transfer dialog over it): only the last one hears the keys and holds focus.
 */
const openDialogs: object[] = [];

/** The elements of `box` that Tab stops at, in order. */
function tabbable(box: HTMLElement): HTMLElement[] {
  const found = box.querySelectorAll<HTMLElement>("a[href], button, input, select, textarea, summary, [tabindex]");
  return Array.from(found).filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled") && el.getClientRects().length > 0,
  );
}

/** Tab from the last element of `box` goes to the first, and Shift+Tab from the first to the last. */
function keepTabInside(event: KeyboardEvent, box: HTMLElement): void {
  const items = tabbable(box);
  if (items.length === 0) {
    event.preventDefault();
    box.focus();
    return;
  }
  const first = items[0];
  const last = items[items.length - 1];
  const active = document.activeElement;
  if (active === null || !box.contains(active)) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  } else if (event.shiftKey && (active === first || active === box)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}

/**
 * Makes `box` a modal dialog while it is mounted: Tab and focus stay inside it, and Escape closes it when `canClose`
 * (and does nothing while a call runs). Escape is answered here and goes no further, so the profile behind the dialog,
 * which closes on Escape too, stays open. While a note of the dialog shows (see `InfoTip`), Escape is the note's: it
 * hides that, and stops the key itself; so is Escape in a control marked `data-takes-escape` (see `ClubSearch`). With
 * `active` false it does none of this, for a box that is a dialog only sometimes (see `ProfileFrame`).
 */
export function useModal(
  box: RefObject<HTMLElement | null>,
  canClose: boolean,
  onClose: () => void,
  active = true,
): void {
  // Kept up to date as soon as a render is on screen, so that a key pressed right after (say) Preview was pressed
  // already finds the dialog busy.
  const latest = useRef({ canClose, onClose });
  // The app does not read the game again by itself while a dialog is up.
  useRefreshHold(active);
  useLayoutEffect(() => {
    latest.current = { canClose, onClose };
  });

  useEffect(() => {
    if (!active) {
      return;
    }
    const self = {};
    openDialogs.push(self);
    const onTop = () => openDialogs[openDialogs.length - 1] === self;
    const onKeyDown = (event: KeyboardEvent) => {
      const dialog = box.current;
      if (!dialog || event.isComposing || !onTop()) {
        return;
      }
      if (event.key === "Escape") {
        // A note that shows, or a control that answers Escape itself (a search, say), takes it: the dialog stays.
        const own = event.target instanceof Element && event.target.closest("[data-takes-escape]");
        if (dialog.querySelector("[data-open-tip]") || own) {
          return;
        }
        event.stopPropagation();
        event.preventDefault();
        if (latest.current.canClose) {
          latest.current.onClose();
        }
      } else if (event.key === "Tab") {
        keepTabInside(event, dialog);
      }
    };
    const onFocusIn = (event: FocusEvent) => {
      const dialog = box.current;
      if (dialog && onTop() && event.target instanceof Node && !dialog.contains(event.target)) {
        (tabbable(dialog)[0] ?? dialog).focus();
      }
    };
    // The window's capture phase comes before anything else: no other handler sees the keys while the dialog is up.
    window.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("focusin", onFocusIn);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("focusin", onFocusIn);
      openDialogs.splice(openDialogs.indexOf(self), 1);
    };
  }, [box, active]);
}
