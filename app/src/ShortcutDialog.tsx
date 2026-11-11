// Choosing the shortcut that shows the overlay (see overlay.ts): a small dialog that takes the keys pressed, at least
// one modifier and one other key, shows them as the system writes shortcuts (⌃⌥⇧⌘E on macOS, Ctrl+Alt+Shift+E
// elsewhere), and has the app register them (`set_overlay_shortcut`): another app may hold them, and then the old
// shortcut stays and the dialog says why. The app keeps the one chosen for its next start.

import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { call } from "./api";
import { describeError } from "./format";
import { shortcutText as text } from "./labels";
import { useModal } from "./useModal";

/** The shortcut the app starts with, as the shortcut plugin reads it. */
export const DEFAULT_SHORTCUT = "Alt+Shift+KeyE";

const MAC = /Mac|iPhone|iPad/.test(navigator.platform);

/** The modifiers, in the order the system writes them, as the plugin reads them and as they are shown. */
const MODIFIERS: readonly { name: string; mac: string; other: string; held: (e: KeyboardEvent) => boolean }[] = [
  { name: "Control", mac: "⌃", other: "Ctrl", held: (e) => e.ctrlKey },
  { name: "Alt", mac: "⌥", other: "Alt", held: (e) => e.altKey },
  { name: "Shift", mac: "⇧", other: "Shift", held: (e) => e.shiftKey },
  { name: "Super", mac: "⌘", other: "Win", held: (e) => e.metaKey },
];

/** The modifiers' names as the user's system shows them, for the prompt. */
export const MODIFIER_NAMES = MODIFIERS.map((m) => (MAC ? m.mac : m.other)).join(" ");

/** A key as it is shown: the letter of `KeyE`, the digit of `Digit1`, or the code itself (`F5`, `Space`). */
function keyName(code: string): string {
  return code.replace(/^Key(?=[A-Z]$)/, "").replace(/^Digit(?=\d$)/, "");
}

/** A shortcut as the plugin reads it (`Alt+Shift+KeyE`), as the user's system shows one: ⌥⇧E, or Alt+Shift+E. */
export function shortcutLabel(shortcut: string): string {
  const parts = shortcut.split("+");
  const key = keyName(parts[parts.length - 1]);
  const held = MODIFIERS.filter((m) => parts.slice(0, -1).some((p) => p.toLowerCase() === m.name.toLowerCase()));
  return MAC ? held.map((m) => m.mac).join("") + key : [...held.map((m) => m.other), key].join("+");
}

/** The keys that are only modifiers, which make no shortcut on their own. */
const MODIFIER_CODES = /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$|^CapsLock$|^Fn/;

/** The shortcut that `event` makes, as the plugin reads it; none while only modifiers are held, or none is. */
function shortcutOf(event: KeyboardEvent): string | null {
  if (MODIFIER_CODES.test(event.code) || event.code === "" || event.code === "Tab" || event.code === "Escape") {
    return null;
  }
  const held = MODIFIERS.filter((m) => m.held(event));
  return held.length === 0 ? null : [...held.map((m) => m.name), event.code].join("+");
}

interface ShortcutDialogProps {
  /** The shortcut now; null when another app holds the one chosen. */
  current: string | null;
  /** Called with the shortcut once it is registered. */
  onChanged: (shortcut: string) => void;
  onClose: () => void;
}

export function ShortcutDialog({ current, onChanged, onClose }: ShortcutDialogProps) {
  const base = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const capture = useRef<HTMLDivElement>(null);
  const [picked, setPicked] = useState<string | null>(null);
  // A key pressed with no modifier, to say what is missing.
  const [bare, setBare] = useState(false);
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useModal(dialog, !saving, onClose);

  useEffect(() => {
    capture.current?.focus();
  }, []);

  const keyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.code === "Tab" || event.code === "Escape") {
      return;
    }
    event.preventDefault();
    const shortcut = shortcutOf(event);
    if (shortcut !== null) {
      setPicked(shortcut);
      setBare(false);
      setFailure(null);
    } else if (!MODIFIER_CODES.test(event.code)) {
      setBare(true);
    }
  };

  const save = async (shortcut: string) => {
    setSaving(true);
    setFailure(null);
    try {
      const registered = await call<string>("set_overlay_shortcut", { shortcut });
      onChanged(registered);
      onClose();
    } catch (e) {
      setFailure(text.failed(describeError(e)));
      setSaving(false);
    }
  };

  return createPortal(
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          event.preventDefault();
        }
      }}
    >
      <div
        ref={dialog}
        className="modal shortcut-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${base}-title`}
        aria-busy={saving || undefined}
        tabIndex={-1}
      >
        <header className="modal-head">
          <h2 id={`${base}-title`}>{text.title}</h2>
        </header>
        <div className="modal-body">
          <p className="note">{current === null ? text.none : text.current(shortcutLabel(current))}</p>
          <div
            ref={capture}
            className="shortcut-capture"
            tabIndex={0}
            role="textbox"
            aria-label={text.prompt(MODIFIER_NAMES)}
            aria-describedby={`${base}-hint`}
            onKeyDown={keyDown}
          >
            {picked === null ? <span className="muted">{text.prompt(MODIFIER_NAMES)}</span> : <kbd>{shortcutLabel(picked)}</kbd>}
          </div>
          <p id={`${base}-hint`} className={bare ? "move-note invalid" : "move-note"} role="status">
            {bare ? text.needsModifier(MODIFIER_NAMES) : text.hint}
          </p>
        </div>
        {failure !== null && (
          <div className="banner move-error" role="alert">
            {failure}
          </div>
        )}
        <footer className="modal-foot">
          <button
            type="button"
            disabled={saving || current === DEFAULT_SHORTCUT}
            onClick={() => void save(DEFAULT_SHORTCUT)}
          >
            {text.reset(shortcutLabel(DEFAULT_SHORTCUT))}
          </button>
          <button
            type="button"
            className="primary"
            disabled={saving || picked === null}
            onClick={() => picked !== null && void save(picked)}
          >
            {saving ? text.saving : text.save}
          </button>
          <button type="button" disabled={saving} onClick={onClose}>
            {text.cancel}
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
