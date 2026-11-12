// A box over a list whose height the user sets by dragging the line under it: drawn up, the box scrolls in itself and
// the list under it takes the room, so that more of its rows show. Dragged down to its full height, or double-clicked,
// it takes the room its content needs again. The arrow keys move the line too, Home to the least and End to full. The
// height is kept between runs, under `storageKey`.

import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";

/** The least the box is drawn up to, in CSS pixels: a line of its controls. */
const LEAST = 44;

/** How far an arrow key moves the line. */
const STEP = 24;

/** The height kept, in CSS pixels; null for the box's full height. */
function savedHeight(key: string): number | null {
  try {
    const saved = Number(window.localStorage.getItem(key));
    return Number.isFinite(saved) && saved >= LEAST ? saved : null;
  } catch {
    return null;
  }
}

function saveHeight(key: string, height: number | null): void {
  try {
    if (height === null) {
      window.localStorage.removeItem(key);
    } else {
      window.localStorage.setItem(key, String(Math.round(height)));
    }
  } catch {
    // Not kept: this run still has it.
  }
}

interface ResizableTopProps {
  /** Where the height is kept. */
  storageKey: string;
  className?: string;
  /** The name of the line, for a screen reader: "Resize the team's controls". */
  label: string;
  /** Its hover text. */
  title: string;
  children: ReactNode;
}

export function ResizableTop({ storageKey, className, label, title, children }: ResizableTopProps) {
  const box = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState<number | null>(() => savedHeight(storageKey));
  const drag = useRef<{ y: number; from: number } | null>(null);

  useEffect(() => saveHeight(storageKey, height), [storageKey, height]);

  /** The height `wanted` comes to: at least `LEAST`, and none (the full height) once it reaches the content's. */
  const fit = (wanted: number): number | null => {
    const full = box.current?.scrollHeight ?? Infinity;
    return wanted >= full ? null : Math.max(LEAST, wanted);
  };

  const shown = () => box.current?.getBoundingClientRect().height ?? 0;

  const down = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { y: event.clientY, from: shown() };
  };

  const move = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current) {
      setHeight(fit(drag.current.from + event.clientY - drag.current.y));
    }
  };

  const up = (event: PointerEvent<HTMLDivElement>) => {
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const key = (event: KeyboardEvent<HTMLDivElement>) => {
    switch (event.key) {
      case "ArrowUp":
        setHeight(fit(shown() - STEP));
        break;
      case "ArrowDown":
        setHeight(fit(shown() + STEP));
        break;
      case "Home":
        setHeight(LEAST);
        break;
      case "End":
        setHeight(null);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  return (
    <>
      <div
        ref={box}
        className={className ? `resizable-top ${className}` : "resizable-top"}
        style={height === null ? undefined : { height, overflow: "auto" }}
      >
        {children}
      </div>
      <div
        className="resize-line"
        role="separator"
        aria-orientation="horizontal"
        aria-label={label}
        title={title}
        tabIndex={0}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onDoubleClick={() => setHeight(null)}
        onKeyDown={key}
      />
    </>
  );
}
