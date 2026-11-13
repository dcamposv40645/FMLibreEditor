import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { call } from "./api";
import { isRecord, restored, useSnapshot } from "./restore";
import { askToLeave } from "./useProfileEditing";

/** Rows shown at once; the best come first. */
const ROW_LIMIT = 200;

/** The search text and the UID of the selected row that a list saved; what is missing or wrong is the list's start. */
function readList(value: unknown): { query: string; selected: number | null } {
  const given = isRecord(value) ? value : {};
  return {
    query: typeof given.query === "string" ? given.query : "",
    selected: typeof given.selected === "number" && Number.isInteger(given.selected) ? given.selected : null,
  };
}

interface Page<Row> {
  total: number;
  rows: Row[];
}

/** What a list tells the actions in its toolbar about itself. */
export interface ListState {
  /** The text in the search box. */
  query: string;
  /**
   * How many rows the search and the filters select: the total of the rows on show. Null until the list has them,
   * and while it is asking again after the search or the filters changed.
   */
  total: number | null;
  /** How many of them the list has rows for: the best ones (see `summary`). */
  shown: number;
}

interface ListBrowserProps<Row extends { uid: number }> {
  /**
   * What the list is called in the snapshot of the window that is kept when the language is switched (see
   * `restore.ts`), which gives it back its search text and its selected row: no other list has the same name.
   */
  name: string;
  /**
   * The Rust command that searches, e.g. "search_players". It takes `{ query, limit, ...requestArgs }` and
   * returns a page.
   */
  command: string;
  /**
   * Arguments for the command besides `query` and `limit`, e.g. `{ filter, order }`. They are compared by
   * value, so the list is asked again when they change however often the object is built anew.
   */
  requestArgs?: Record<string, unknown>;
  /** Changes when the game data is read again, so the list and the open profile are too. */
  version: number;
  /**
   * Every list stays mounted while another one is shown, so it keeps its query, rows and selection.
   * Only the active one is visible, takes focus when it becomes active, and shows its profile.
   */
  active: boolean;
  placeholder: string;
  /** The line beside the search box. */
  summary: (total: number, shown: number) => string;
  /** The message under an empty table. */
  empty: (query: string) => string;
  /** The filters of the list, between the search box and the table. */
  filterBar?: ReactNode;
  /** Buttons at the end of the toolbar, e.g. Mass edit; they get the search text and the total. */
  toolbarActions?: (list: ListState) => ReactNode;
  /** Under the table, e.g. the tray of the players picked for a comparison; it takes no room while it is empty. */
  footer?: ReactNode;
  /** An extra class of the table, for column rules of that list. */
  tableClass?: string;
  /** The header cells. */
  header: ReactNode;
  /** The cells of one row. */
  cells: (row: Row) => ReactNode;
  /** The profile panel of the selected row. */
  profile: (row: Row, panel: { version: number; onClose: () => void }) => ReactNode;
  /** A row to select from outside the list: what FM's screen shows (see followView.ts). */
  followed?: Followed<Row> | null;
}

/** A row to select from outside the list, and which time it is. */
export interface Followed<Row> {
  row: Row;
  seq: number;
}

/** A searchable table with the profile of the selected row beside it. */
export function ListBrowser<Row extends { uid: number }>(props: ListBrowserProps<Row>) {
  const { command, requestArgs, version, active, placeholder, summary, empty, filterBar, toolbarActions } = props;
  const { name, footer, tableClass, header, cells, profile, followed } = props;
  // What the list held before the language was switched, if it was: the search text, and the UID of the row to select
  // as soon as the rows are there again.
  const [start] = useState(() => restored(name, readList));
  const [query, setQuery] = useState(start.query);
  const wanted = useRef(start.selected);
  const [page, setPage] = useState<Page<Row> | null>(null);
  // The search text and arguments the rows on show were asked for: their total is the list's only while those are
  // the ones in the search box and the filters now.
  const [shownFor, setShownFor] = useState<{ query: string; argsKey: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Row | null>(null);
  useSnapshot(name, () => ({ query, selected: selected?.uid ?? null }));
  const searchInput = useRef<HTMLInputElement>(null);
  const tableWrap = useRef<HTMLDivElement>(null);
  const selectedRow = useRef<HTMLTableRowElement>(null);

  // The arguments as JSON, which is how they reach the command: the effect below depends on their value.
  const argsKey = JSON.stringify(requestArgs ?? {});
  // The arguments of the rows on show, so a new sort or filter can start at the top.
  const shownArgs = useRef(argsKey);

  const closeProfile = useCallback(() => {
    // The panel unmounts with the focused control inside it; keep keyboard focus in the table.
    if (document.activeElement?.closest(".profile")) {
      selectedRow.current?.focus();
    }
    setSelected(null);
  }, []);

  // Another row replaces the profile, and with it the edits pending in it: the profile asks first.
  const pick = (row: Row) => (row.uid === selected?.uid ? setSelected(row) : askToLeave(() => setSelected(row)));

  useEffect(() => {
    let cancelled = false;
    const args: Record<string, unknown> = JSON.parse(argsKey);
    // Typing in the search box or a filter waits a little, so a burst of keys makes one request.
    // A list nobody has touched yet loads at once.
    const untouched = query === "" && argsKey === "{}";
    const timer = setTimeout(
      async () => {
        try {
          const result = await call<Page<Row>>(command, { ...args, query, limit: ROW_LIMIT });
          if (!cancelled) {
            setPage(result);
            setShownFor({ query, argsKey });
            setError(null);
            if (shownArgs.current !== argsKey) {
              // A new sort or filter starts at the top; reading the same list again keeps its place.
              shownArgs.current = argsKey;
              tableWrap.current?.scrollTo({ top: 0 });
            }
            // The first rows decide: the row that was selected before the language was switched is selected again if
            // they have it, and not later, when the user has moved on.
            const uid = wanted.current;
            if (uid !== null) {
              wanted.current = null;
              const row = result.rows.find((candidate) => candidate.uid === uid);
              if (row !== undefined) {
                setSelected(row);
              }
            }
          }
        } catch (e) {
          if (!cancelled) {
            setError(String(e));
          }
        }
      },
      untouched ? 0 : 150,
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [command, query, argsKey, version]);

  useEffect(() => {
    if (active) {
      searchInput.current?.focus();
    }
  }, [active]);

  // What FM's screen shows is selected, whether or not its row is on show. Nothing is pending then: the profile being
  // edited, or a dialog up, holds it back until they are done (see followView.ts).
  const followedSeq = followed?.seq;
  const followedRow = useRef(followed?.row);
  followedRow.current = followed?.row;
  useEffect(() => {
    if (followedSeq !== undefined && followedRow.current !== undefined) {
      setSelected(followedRow.current);
    }
  }, [followedSeq]);

  const settled = page !== null && shownFor !== null && shownFor.query === query && shownFor.argsKey === argsKey;

  // A hidden list has no profile open: its window-wide Escape handler must not fire.
  const showProfile = active && selected !== null;
  const className = ["workspace", showProfile && "with-profile", !active && "inactive"].filter(Boolean).join(" ");

  return (
    <div className={className}>
      <section className="players">
        <div className="toolbar">
          <label className="search">
            <svg viewBox="0 0 20 20" aria-hidden="true">
              <circle cx="8.5" cy="8.5" r="5.5" />
              <path d="m13 13 4.5 4.5" />
            </svg>
            <input
              ref={searchInput}
              value={query}
              onChange={(e) => setQuery(e.currentTarget.value)}
              placeholder={placeholder}
              spellCheck={false}
            />
          </label>
          <span className="count">{page ? summary(page.total, page.rows.length) : ""}</span>
          {toolbarActions?.({ query, total: settled ? page.total : null, shown: page?.rows.length ?? 0 })}
        </div>
        {filterBar}
        {error && <div className="banner">{error}</div>}

        <div className="table-wrap" ref={tableWrap}>
          <table className={tableClass ? `grid ${tableClass}` : "grid"}>
            <thead>
              <tr>{header}</tr>
            </thead>
            <tbody>
              {page?.rows.map((row) => {
                const isSelected = row.uid === selected?.uid;
                return (
                  <tr
                    key={row.uid}
                    ref={isSelected ? selectedRow : undefined}
                    className={isSelected ? "selected" : undefined}
                    aria-current={isSelected ? "true" : undefined}
                    tabIndex={0}
                    onClick={() => pick(row)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        pick(row);
                      }
                    }}
                  >
                    {cells(row)}
                  </tr>
                );
              })}
            </tbody>
          </table>
          {page && page.rows.length === 0 && <p className="empty">{empty(query)}</p>}
        </div>
        {footer}
      </section>

      {showProfile && (
        <Fragment key={selected.uid}>{profile(selected, { version, onClose: closeProfile })}</Fragment>
      )}
    </div>
  );
}
