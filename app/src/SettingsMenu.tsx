// The settings menu at the right of the masthead: a button with a gear, which opens a menu of the languages, each with
// its flag and its own name, and of the currencies the app can show money in, each with its sign and its name. The
// menu is drawn over the window under its button, as the captain menu of a squad row is (see CaptainMenu.tsx), so that
// the masthead's styles stay off it.

import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { call } from "./api";
import { CURRENCIES, currency, setCurrency, type Currency } from "./currency";
import { currencyName, currencySymbolOf } from "./format";
import { locale, setLocale, type Locale } from "./i18n";
import { appText } from "./labels";
import { ShortcutDialog, shortcutLabel } from "./ShortcutDialog";
import { askToLeave } from "./useProfileEditing";
import { ZOOM_LEVELS, onZoom, setZoom, stepZoom, zoomKeys, zoomLevel, zoomPercent, zooms } from "./zoom";

/** The room the menu keeps between itself and its button. */
const GAP = 4;

/**
 * The flag of each language, in a box of 3:2: the Union Jack for English, as FM is British (cut to 3:2 at the sides,
 * as flag sets do), and the flags of Turkey, Spain, Indonesia and Italy whole. Spain's is drawn without the coat of
 * arms, as its civil flag is: at this size the arms would be a smudge. Indonesia's is 2:3 in law; drawn at 3:2 like the
 * others. Italy's is the tricolour, three vertical bands.
 */
const FLAGS: Record<Locale, ReactNode> = {
  en: (
    <svg className="language-flag" viewBox="7.5 0 45 30" aria-hidden="true">
      <rect width="60" height="30" fill="#012169" />
      <path d="M0 0l60 30M60 0L0 30" stroke="#fff" strokeWidth="6" />
      <path
        d="M30 15L0 0v2.236L25.528 15zm0 0l30 15v-2.236L34.472 15zm0 0L60 0h-4.472L30 12.764zm0 0L0 30h4.472L30 17.236z"
        fill="#c8102e"
      />
      <path d="M30 0v30M0 15h60" stroke="#fff" strokeWidth="10" />
      <path d="M30 0v30M0 15h60" stroke="#c8102e" strokeWidth="6" />
    </svg>
  ),
  tr: (
    <svg className="language-flag" viewBox="0 0 90 60" aria-hidden="true">
      <rect width="90" height="60" fill="#e30a17" />
      <g fill="#fff">
        <path d="M42.675 38.021a15 15 0 1 1 0-16.042 12 12 0 1 0 0 16.042z" />
        <path d="M41.75 30l13.568-4.408-8.386 11.541V22.867l8.386 11.541z" />
      </g>
    </svg>
  ),
  es: (
    <svg className="language-flag" viewBox="0 0 3 2" aria-hidden="true">
      <rect width="3" height="2" fill="#aa151b" />
      <rect y="0.5" width="3" height="1" fill="#f1bf00" shapeRendering="crispEdges" />
    </svg>
  ),
  id: (
    <svg className="language-flag" viewBox="0 0 3 2" aria-hidden="true">
      <rect width="3" height="2" fill="#fff" />
      <rect width="3" height="1" fill="#ce1126" shapeRendering="crispEdges" />
    </svg>
  ),
  it: (
    <svg className="language-flag" viewBox="0 0 3 2" aria-hidden="true">
      <rect width="3" height="2" fill="#fff" />
      <rect width="1" height="2" fill="#009246" shapeRendering="crispEdges" />
      <rect x="2" width="1" height="2" fill="#ce2b37" shapeRendering="crispEdges" />
    </svg>
  ),
};

/** The languages on offer, in the order of the menu, each with its own name. */
const LANGUAGES: readonly { id: Locale; name: string }[] = [
  { id: "en", name: "English" },
  { id: "tr", name: "Türkçe" },
  { id: "es", name: "Español" },
  { id: "id", name: "Bahasa Indonesia" },
  { id: "it", name: "Italiano" },
];

/** The name and hover text of the button: "Settings: English, Euro". */
const BUTTON_LABEL = `${appText.settingsLabel}: ${LANGUAGES.find(({ id }) => id === locale)?.name ?? locale}, ${currencyName()}`;

/** Where the open menu hangs: the window's coordinates of its button, from the right edge and the top. */
interface Anchor {
  right: number;
  bottom: number;
}

const CHECK = (
  <svg className="settings-check" viewBox="0 0 20 20" aria-hidden="true">
    <path d="m4.5 10.5 3.5 3.5 7.5-8" />
  </svg>
);

/**
 * The button and, once it is pressed, its menu. Choosing another language or currency reloads the window (see
 * `setLocale` and `setCurrency`), which the app survives connected (see `current_connection`), so edits pending in an
 * open profile are asked about first. While the app is `busy` the button waits.
 */
interface SettingsMenuProps {
  busy: boolean;
  /** Whether the app reads the game again by itself once it moves on (see autoRefresh.ts), and what changes that. */
  autoRefresh: boolean;
  onAutoRefresh: (on: boolean) => void;
  /** Whether the app opens the profile of what FM's screen shows (see followView.ts), and what changes that. */
  followView: boolean;
  onFollowView: (on: boolean) => void;
  /** Shows the overlay over the game, or hides it (see overlay.ts); none where there is no overlay (a browser). */
  onOverlay?: () => void;
}

export function SettingsMenu(props: SettingsMenuProps) {
  const { busy, autoRefresh, onAutoRefresh, followView, onFollowView, onOverlay } = props;
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const open = anchor !== null;

  // The overlay's shortcut, read each time the menu opens: undefined until then, null when another app holds it.
  const [shortcut, setShortcut] = useState<string | null | undefined>(undefined);
  const [choosingShortcut, setChoosingShortcut] = useState(false);

  // The window's zoom, which its own shortcuts change too (see zoom.ts).
  const [zoom, setZoomShown] = useState(zoomLevel);
  useEffect(() => onZoom(setZoomShown), []);

  // Where the button is now: the menu hangs under it.
  const anchorOfButton = (): Anchor | null => {
    const box = button.current?.getBoundingClientRect();
    return box ? { right: document.documentElement.clientWidth - box.right, bottom: box.bottom } : null;
  };

  const show = () => {
    if (onOverlay) {
      void call<string | null>("overlay_shortcut").then(setShortcut, () => setShortcut(null));
    }
    const at = anchorOfButton();
    if (at) {
      setAnchor(at);
    }
  };

  const close = () => {
    setAnchor(null);
    button.current?.focus();
  };

  // Once the menu is on screen, the language in use takes focus, so that the arrow keys go on from it.
  useLayoutEffect(() => {
    if (anchor !== null) {
      menu.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
    }
  }, [anchor]);

  // A press elsewhere closes it. A resize, a change of zoom too, moves the button: the menu follows it.
  useEffect(() => {
    if (!open) {
      return;
    }
    const pressed = (event: Event) => {
      const target = event.target;
      if (target instanceof Node && (menu.current?.contains(target) || button.current?.contains(target))) {
        return;
      }
      setAnchor(null);
    };
    const resized = () => setAnchor(anchorOfButton());
    document.addEventListener("pointerdown", pressed, true);
    window.addEventListener("resize", resized);
    return () => {
      document.removeEventListener("pointerdown", pressed, true);
      window.removeEventListener("resize", resized);
    };
  }, [open]);

  const chooseLanguage = (next: Locale) => {
    close();
    if (!busy && next !== locale) {
      askToLeave(() => setLocale(next));
    }
  };

  const chooseCurrency = (next: Currency) => {
    close();
    if (!busy && next !== currency) {
      askToLeave(() => setCurrency(next));
    }
  };

  // The arrow keys go through all the lists, as one.
  const keyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>(
        '[role="menuitemradio"], [role="menuitemcheckbox"], [role="menuitem"]:not(:disabled)',
      ),
    );
    const at = items.findIndex((item) => item === document.activeElement);
    let next: HTMLElement | undefined;
    switch (event.key) {
      case "ArrowDown":
        next = items[(at + 1) % items.length];
        break;
      case "ArrowUp":
        next = items[(at - 1 + items.length) % items.length];
        break;
      case "Home":
        next = items[0];
        break;
      case "End":
        next = items[items.length - 1];
        break;
      case "Escape":
        // An open profile closes on Escape too: this one is the menu's.
        event.stopPropagation();
        event.preventDefault();
        close();
        return;
      case "Tab":
        // Tab leaves the menu for the controls after its button.
        close();
        return;
      default:
        return;
    }
    event.preventDefault();
    next?.focus();
  };

  return (
    <>
      <button
        ref={button}
        type="button"
        className="settings-toggle"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={BUTTON_LABEL}
        title={BUTTON_LABEL}
        disabled={busy}
        onClick={() => (open ? setAnchor(null) : show())}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && !open) {
            event.preventDefault();
            show();
          }
        }}
      >
        <svg className="settings-gear" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.08a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.08a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
        <svg className="settings-chevron" viewBox="0 0 20 20" aria-hidden="true">
          <path d="m5 8 5 5 5-5" />
        </svg>
      </button>
      {anchor !== null &&
        createPortal(
          <div
            ref={menu}
            className="settings-menu"
            role="menu"
            aria-label={appText.settingsLabel}
            style={{ top: anchor.bottom + GAP, right: anchor.right }}
            onKeyDown={keyDown}
          >
            <div role="group" aria-labelledby={`${id}-language`}>
              <div id={`${id}-language`} className="settings-heading">
                {appText.languageLabel}
              </div>
              {LANGUAGES.map(({ id: language, name }) => (
                <button
                  key={language}
                  type="button"
                  role="menuitemradio"
                  aria-checked={language === locale}
                  lang={language}
                  tabIndex={-1}
                  onClick={() => chooseLanguage(language)}
                >
                  {FLAGS[language]}
                  {name}
                  {language === locale && CHECK}
                </button>
              ))}
            </div>
            <div role="group" aria-labelledby={`${id}-currency`}>
              <div id={`${id}-currency`} className="settings-heading">
                {appText.currencyLabel}
              </div>
              {CURRENCIES.map((code) => (
                <button
                  key={code}
                  type="button"
                  role="menuitemradio"
                  aria-checked={code === currency}
                  tabIndex={-1}
                  onClick={() => chooseCurrency(code)}
                >
                  <span className="currency-sign" aria-hidden="true">
                    {currencySymbolOf(code)}
                  </span>
                  {currencyName(code)}
                  {code === currency && CHECK}
                </button>
              ))}
            </div>
            {zooms && (
              <div role="group" aria-labelledby={`${id}-zoom`}>
                <div id={`${id}-zoom`} className="settings-heading">
                  {appText.zoomLabel}
                </div>
                <div className="settings-zoom">
                  <button
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    aria-label={appText.zoomOut}
                    title={`${appText.zoomOut} (${zoomKeys("−")})`}
                    disabled={zoom === ZOOM_LEVELS[0]}
                    onClick={() => stepZoom(-1)}
                  >
                    −
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    aria-label={`${zoomPercent(zoom)}: ${appText.zoomReset}`}
                    title={`${appText.zoomReset} (${zoomKeys("0")})`}
                    onClick={() => setZoom(1)}
                  >
                    {zoomPercent(zoom)}
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    aria-label={appText.zoomIn}
                    title={`${appText.zoomIn} (${zoomKeys("+")})`}
                    disabled={zoom === ZOOM_LEVELS[ZOOM_LEVELS.length - 1]}
                    onClick={() => stepZoom(1)}
                  >
                    +
                  </button>
                </div>
              </div>
            )}
            <div role="group" aria-labelledby={`${id}-game`}>
              <div id={`${id}-game`} className="settings-heading">
                {appText.gameLabel}
              </div>
              <button
                type="button"
                role="menuitemcheckbox"
                aria-checked={autoRefresh}
                tabIndex={-1}
                title={appText.followGameTitle}
                onClick={() => onAutoRefresh(!autoRefresh)}
              >
                {appText.followGame}
                {autoRefresh && CHECK}
              </button>
              <button
                type="button"
                role="menuitemcheckbox"
                aria-checked={followView}
                tabIndex={-1}
                title={appText.followScreenTitle}
                onClick={() => onFollowView(!followView)}
              >
                {appText.followScreen}
                {followView && CHECK}
              </button>
              {onOverlay && (
                <button
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  title={appText.overlayTitle(shortcut ? shortcutLabel(shortcut) : null)}
                  onClick={() => {
                    close();
                    onOverlay();
                  }}
                >
                  {appText.overlay}
                  {shortcut && <kbd className="settings-shortcut">{shortcutLabel(shortcut)}</kbd>}
                </button>
              )}
              {onOverlay && (
                <button
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  aria-haspopup="dialog"
                  onClick={() => {
                    close();
                    setChoosingShortcut(true);
                  }}
                >
                  {appText.overlayShortcut}
                </button>
              )}
            </div>
          </div>,
          document.body,
        )}
      {choosingShortcut && (
        <ShortcutDialog
          current={shortcut ?? null}
          onChanged={setShortcut}
          onClose={() => {
            setChoosingShortcut(false);
            button.current?.focus();
          }}
        />
      )}
    </>
  );
}
