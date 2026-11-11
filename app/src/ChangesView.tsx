// The Changes view: everything written to the game this session, newest first, with an Undo for each change. A mass
// edit is one row, with an Undo of the whole mass edit and a toggle that opens its changes. One mass edit can log
// thousands of changes, so the list, and the changes of a mass edit that is open, are shown a page at a time.

import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { clauseLine } from "./ClauseList";
import { valueText } from "./editDraft";
import { changeLog, dateFromNumber, isStoredFiner, shownValue, undoBatch, undoChange } from "./editing";
import { fieldTable } from "./fieldTables";
import { describeError, formatDate, formatDateTime, formatMoney, formatTime } from "./format";
import {
  changesText as text,
  fieldLabel,
  jobLabel,
  massEditText,
  profileText,
  swapNote,
  teamLabel,
  valueChange,
} from "./labels";
import { undoFailureLines } from "./massDraft";
import { ruleLabel } from "./TiesGroup";
import type { Clause, EditField, Entity, LoggedChange } from "./types";

/**
 * This session's change log. `reload` reads it again; when several reads overlap, the answer to the last one asked
 * is the one kept. `clear` forgets it, as a new connection starts with an empty log.
 */
export function useChangeLog() {
  const [log, setLog] = useState<LoggedChange[]>([]);
  const [error, setError] = useState<string | null>(null);
  const asked = useRef(0);

  const reload = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const entries = await changeLog();
      if (mine === asked.current) {
        setLog(entries);
        setError(null);
      }
    } catch (e) {
      if (mine === asked.current) {
        setError(describeError(e));
      }
    }
  }, []);

  const clear = useCallback(() => {
    asked.current++;
    setLog([]);
    setError(null);
  }, []);

  return { log, error, reload, clear };
}

/**
 * The editable fields of players, staff and clubs by `entity:key`, to turn stored values into shown ones and to
 * know what kind of value they are (a date, an amount of money); null until read.
 */
function useFieldIndex(): { fields: ReadonlyMap<string, EditField> | null; error: string | null } {
  const [fields, setFields] = useState<ReadonlyMap<string, EditField> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([fieldTable("player"), fieldTable("staff"), fieldTable("club")]).then(
      (tables) => {
        if (!cancelled) {
          setFields(new Map(tables.flat().map((field) => [`${field.entity}:${field.key}`, field] as const)));
        }
      },
      (e) => {
        if (!cancelled) {
          setError(describeError(e));
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  return { fields, error };
}

/** Rows shown at once: of the list, and of the changes of a mass edit that is open. */
const PAGE_SIZE = 100;

/** The columns of the table, for a cell that spans them all. */
const COLUMNS = 5;

/** How many pages `total` rows take. */
const pageCount = (total: number) => Math.ceil(total / PAGE_SIZE);

/** `page` (from 0), kept within the pages `total` rows take: the list may have become shorter than it was. */
const withinPages = (page: number, total: number) => Math.min(page, Math.max(0, pageCount(total) - 1));

/** The rows of `page` (from 0). */
function pageOf<T>(rows: readonly T[], page: number): T[] {
  return rows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
}

/** What the log says about one mass edit. */
interface Batch {
  id: number;
  /** What it changed: a mass edit is made in one list. */
  entity: Entity;
  /** Its changes, newest first, as the log has them. */
  changes: LoggedChange[];
  /** How many of them are not undone. */
  remaining: number;
  /** The names of the fields it changed, each once, in the order they were written. */
  fields: string[];
}

/** A row of the list: a change that was made on its own, or a whole mass edit. */
type Entry = { kind: "change"; change: LoggedChange } | { kind: "batch"; batch: Batch };

/**
 * The rows of the list for `log` (newest change first, as the log is): the changes made on their own, and each mass
 * edit once, where its newest change is.
 */
function entriesOf(log: readonly LoggedChange[]): Entry[] {
  const entries: Entry[] = [];
  const batches = new Map<number, Batch>();
  for (const change of log) {
    if (change.batch === null) {
      entries.push({ kind: "change", change });
      continue;
    }
    let batch = batches.get(change.batch);
    if (batch === undefined) {
      batch = { id: change.batch, entity: change.entity, changes: [], remaining: 0, fields: [] };
      batches.set(change.batch, batch);
      entries.push({ kind: "batch", batch });
    }
    batch.changes.push(change);
    if (change.undoneAt === null) {
      batch.remaining++;
    }
  }
  for (const batch of batches.values()) {
    batch.fields = fieldNames(batch);
  }
  return entries;
}

/** The names of the fields `batch` changed, each once, the oldest change first: the order the mass edit had them in. */
function fieldNames(batch: Batch): string[] {
  const names = new Set<string>();
  for (let at = batch.changes.length - 1; at >= 0; at--) {
    names.add(fieldLabel(batch.entity, batch.changes[at].field));
  }
  return [...names];
}

/** What undoing a whole mass edit came to: how many changes were taken back and what could not be, or why it failed. */
type BatchOutcome = { error: string } | { undone: number; problems: string[] };

interface ChangesViewProps {
  log: LoggedChange[];
  /** Why the log could not be read. */
  error: string | null;
  /** Reads the log again. */
  reload: () => void;
  /** Called after a change was undone: the game's data and the log are not what they were. */
  onUndone: () => void;
}

/** The table of this session's changes. It reads the log again whenever it is shown. */
export function ChangesView({ log, error, reload, onUndone }: ChangesViewProps) {
  const { fields, error: fieldsError } = useFieldIndex();
  const [undoing, setUndoing] = useState(false);
  // Why an undo failed, by change id.
  const [failures, setFailures] = useState<Record<number, string>>({});
  // What undoing a whole mass edit did, by batch: it is shown on the row of the mass edit.
  const [outcomes, setOutcomes] = useState<Record<number, BatchOutcome>>({});
  // The page of the list that is on show, from 0.
  const [page, setPage] = useState(0);
  // The mass edits that are open, by batch, each with the page of its changes that is on show, from 0.
  const [open, setOpen] = useState<Partial<Record<number, number>>>({});
  const entries = useMemo(() => entriesOf(log), [log]);
  const tableWrap = useRef<HTMLDivElement>(null);

  useEffect(() => {
    reload();
  }, [reload]);

  const undo = async (change: LoggedChange) => {
    setUndoing(true);
    setFailures((old) => {
      const rest = { ...old };
      delete rest[change.id];
      return rest;
    });
    try {
      await undoChange(change.id);
      onUndone();
    } catch (e) {
      setFailures((old) => ({ ...old, [change.id]: describeError(e) }));
      // The log may be out of date, e.g. when the change was undone already.
      reload();
    } finally {
      setUndoing(false);
    }
  };

  const undoWholeBatch = async (batch: number) => {
    setUndoing(true);
    setOutcomes((old) => {
      const rest = { ...old };
      delete rest[batch];
      return rest;
    });
    try {
      const done = await undoBatch(batch);
      const problems = undoFailureLines(done.failed, log);
      setOutcomes((old) => ({ ...old, [batch]: { undone: done.undone, problems } }));
      if (done.undone > 0) {
        onUndone();
      } else {
        // The log may be out of date, e.g. when the changes were undone one by one already.
        reload();
      }
    } catch (e) {
      setOutcomes((old) => ({ ...old, [batch]: { error: describeError(e) } }));
      reload();
    } finally {
      setUndoing(false);
    }
  };

  const showPage = (next: number) => {
    setPage(next);
    // A page starts at its top.
    tableWrap.current?.scrollTo({ top: 0 });
  };

  /** Opens the mass edit at the first page of its changes, or closes it. */
  const toggle = (batch: number) =>
    setOpen((old) => {
      const rest = { ...old };
      if (old[batch] === undefined) {
        rest[batch] = 0;
      } else {
        delete rest[batch];
      }
      return rest;
    });

  const showBatchPage = (batch: number, next: number) => setOpen((old) => ({ ...old, [batch]: next }));

  // Stored values are turned into shown ones with the field tables, so the rows wait for them.
  const ready = fields !== null || fieldsError !== null;
  const undone = log.filter((change) => change.undoneAt !== null).length;
  const problem = error ?? fieldsError;
  const shownPage = withinPages(page, entries.length);

  const changeRow = (change: LoggedChange, inBatch: boolean) => (
    <ChangeRow
      key={change.id}
      change={change}
      field={fields?.get(`${change.entity}:${change.field}`)}
      failure={failures[change.id]}
      undoing={undoing}
      onUndo={undo}
      inBatch={inBatch}
    />
  );

  return (
    <div className="workspace">
      <section className="players changes">
        <div className="toolbar">
          <h2 className="view-title">{text.title}</h2>
          <span className="count">{text.count(log.length, undone)}</span>
        </div>
        <p className="view-note">{text.hint}</p>
        <p className="view-note">{text.continueHint}</p>
        {problem && (
          <div className="banner" role="alert">
            {problem}
          </div>
        )}

        <div className="table-wrap" ref={tableWrap}>
          <table className="grid changes-grid">
            <thead>
              <tr>
                <th>{text.columns.time}</th>
                <th>{text.columns.name}</th>
                <th>{text.columns.field}</th>
                <th>{text.columns.change}</th>
                <th>{text.columns.status}</th>
              </tr>
            </thead>
            <tbody>
              {ready &&
                pageOf(entries, shownPage).map((entry) => {
                  if (entry.kind === "change") {
                    return changeRow(entry.change, false);
                  }
                  const { batch } = entry;
                  // Undefined while the mass edit is closed.
                  const opened = open[batch.id];
                  const batchPage = opened === undefined ? 0 : withinPages(opened, batch.changes.length);
                  return (
                    <Fragment key={`batch-${batch.id}`}>
                      <BatchRow
                        batch={batch}
                        open={opened !== undefined}
                        undoing={undoing}
                        outcome={outcomes[batch.id]}
                        onToggle={toggle}
                        onUndo={undoWholeBatch}
                      />
                      {opened !== undefined && (
                        <>
                          {pageCount(batch.changes.length) > 1 && (
                            <tr className="batch-pager">
                              <td colSpan={COLUMNS}>
                                <Pager
                                  label={text.batchPages}
                                  page={batchPage}
                                  total={batch.changes.length}
                                  onPage={(next) => showBatchPage(batch.id, next)}
                                />
                              </td>
                            </tr>
                          )}
                          {pageOf(batch.changes, batchPage).map((change) => changeRow(change, true))}
                        </>
                      )}
                    </Fragment>
                  );
                })}
            </tbody>
          </table>
          {ready && log.length === 0 && <p className="empty">{text.empty}</p>}
        </div>
        {ready && pageCount(entries.length) > 1 && (
          <div className="pager-bar">
            <Pager label={text.pages} page={shownPage} total={entries.length} onPage={showPage} />
          </div>
        )}
      </section>
    </div>
  );
}

interface PagerProps {
  /** What the buttons page through, for assistive technology. */
  label: string;
  /** The page that is on show, from 0, and how many rows there are in all. */
  page: number;
  total: number;
  onPage: (page: number) => void;
}

/** Previous and Next with the rows on show between them ("101–200 of 450"); nothing while the rows fit one page. */
function Pager({ label, page, total, onPage }: PagerProps) {
  const box = useRef<HTMLDivElement>(null);
  const pages = pageCount(total);

  // A disabled button cannot keep focus: when the button that was pressed has reached the first or the last page, focus
  // goes to the other one.
  useEffect(() => {
    const active = document.activeElement;
    if (box.current?.contains(active) && active instanceof HTMLElement && active.matches(":disabled")) {
      box.current.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
    }
  }, [page]);

  if (pages <= 1) {
    return null;
  }
  const first = page * PAGE_SIZE + 1;
  const last = Math.min(total, first + PAGE_SIZE - 1);

  return (
    <div ref={box} className="pager" role="group" aria-label={label}>
      <button type="button" disabled={page === 0} onClick={() => onPage(page - 1)}>
        {text.previous}
      </button>
      {/* A live region, so that the new range is read out when a page changes. */}
      <span className="pager-range" role="status">
        {text.range(first, last, total)}
      </span>
      <button type="button" disabled={page >= pages - 1} onClick={() => onPage(page + 1)}>
        {text.next}
      </button>
    </div>
  );
}

interface BatchRowProps {
  batch: Batch;
  /** Whether its changes are shown under it. */
  open: boolean;
  /** Whether any undo is under way: one at a time. */
  undoing: boolean;
  /** What undoing the whole mass edit did. */
  outcome: BatchOutcome | undefined;
  onToggle: (batch: number) => void;
  onUndo: (batch: number) => void;
}

/** A mass edit as one row: what it changed, how much of it is still in place, its Undo and the toggle for its changes. */
function BatchRow({ batch, open, undoing, outcome, onToggle, onUndo }: BatchRowProps) {
  // The newest change gives the time: the mass edit is as old as its last write.
  const newest = batch.changes[0];

  return (
    <tr className={batch.remaining === 0 ? "batch-row undone" : "batch-row"}>
      <td className="date" title={formatDateTime(newest.at)}>
        {formatTime(newest.at)}
      </td>
      <td className="col-person col-batch" colSpan={3}>
        <div className="name">
          {batch.fields.join(", ")}
          <span className="tag kind">{text.kinds[batch.entity]}</span>
          <span className="tag batch">{text.batch}</span>
        </div>
        <div className="sub">{text.batchCount(batch.changes.length, batch.remaining)}</div>
      </td>
      <td className="col-status">
        <div className="row-actions">
          {batch.remaining > 0 && (
            <button
              type="button"
              className="batch-undo"
              disabled={undoing}
              title={text.undoBatchTitle(batch.remaining)}
              onClick={() => onUndo(batch.id)}
            >
              {text.undoBatch}
            </button>
          )}
          <button type="button" className="batch-toggle" aria-expanded={open} onClick={() => onToggle(batch.id)}>
            {open ? text.hideChanges : text.showChanges}
          </button>
        </div>
        {outcome !== undefined && <BatchOutcomeNote outcome={outcome} />}
      </td>
    </tr>
  );
}

interface ChangeRowProps {
  change: LoggedChange;
  /**
   * The field's entry in the field table, for the conversion of its values and to tell a date or an amount of
   * money from a plain number; none when the table lacks it.
   */
  field: EditField | undefined;
  /** Why the last try to undo it failed. */
  failure: string | undefined;
  /** Whether any undo is under way: one at a time. */
  undoing: boolean;
  onUndo: (change: LoggedChange) => void;
  /** Whether it is shown under the row of the mass edit it is part of. */
  inBatch: boolean;
}

/**
 * What a captaincy says under its values: the team, unless it is the first one, and when the two captains swapped, who
 * holds the other role now. The log knows the team only by its place in the club's list, so it is named by that.
 */
function captaincyNotes({ captaincy, field }: LoggedChange): string[] {
  if (!captaincy) {
    return [];
  }
  const notes: string[] = [];
  if (captaincy.team !== 0) {
    notes.push(teamLabel({ index: captaincy.team, kind: captaincy.kind, ageLimit: captaincy.ageLimit }));
  }
  if (captaincy.swapped) {
    notes.push(swapNote(field === "captain" ? "captain" : "viceCaptain", captaincy.from));
  }
  return notes;
}

/** One side of a change to a player's tie: the nation or club, and a training's rule; "None" for no tie. */
function tieEnd(end: { name: string; rule: number | null } | null): string {
  if (end === null) {
    return profileText.none;
  }
  return end.rule === null ? end.name : `${end.name} · ${ruleLabel(end.rule)}`;
}

/** One side of a change to a contract clause: its name, what it depends on and its amount; "None" for no clause. */
function clauseEnd(clause: Clause | null): string {
  return clause === null ? profileText.none : clauseLine(clause);
}

function ChangeRow({ change, field, failure, undoing, onUndo, inBatch }: ChangeRowProps) {
  const label = fieldLabel(change.entity, change.field);
  const shown = (stored: number) => (field ? shownValue(field, stored) : stored);
  // An attribute or a percentage is stored on a finer scale than it is shown: its hover text has the values as stored.
  const storedNote = isStoredFiner(field) ? text.stored(valueChange(change.old, change.new)) : undefined;
  const className = [change.undoneAt !== null && "undone", inBatch && "in-batch"].filter(Boolean).join(" ");
  // A move to another club is logged with the clubs' UIDs as its values: their names say more (a release has no new
  // club: he became a free agent), and so do the players' for a captaincy. A heal is logged as a change of his
  // injuries from how many he had to none, and lifting his bans as one of his bans: "Healed 2 injuries" says it better
  // than "2 → 0". Keeping him from retiring is logged with the date he was to retire on, which it names.
  // No club is a free agent's for a player, and unemployed for a member of staff.
  const noClub = change.entity === "staff" ? profileText.unemployed : profileText.freeAgent;
  const value = change.moved
    ? valueChange(change.moved.from ?? noClub, change.moved.to ?? noClub)
    : change.captaincy
      ? valueChange(change.captaincy.from ?? profileText.none, change.captaincy.to)
      : change.tie
        ? valueChange(tieEnd(change.tie.from), tieEnd(change.tie.to))
      : change.clause
        ? valueChange(clauseEnd(change.clause.from), clauseEnd(change.clause.to))
      : change.stadium
        ? valueChange(change.stadium.from, change.stadium.to)
      : change.field === "injuries"
        ? text.healed(change.old)
        : change.field === "bans"
          ? text.lifted(change.old)
          : change.field === "retirement"
            ? text.keptPlaying(change.old === 0 ? null : formatDate(dateFromNumber(change.old)))
          : change.field === "contract.job"
            ? valueChange(jobLabel(change.old), jobLabel(change.new))
            : valueChange(valueText(field, shown(change.old)), valueText(field, shown(change.new)));

  return (
    <tr className={className || undefined}>
      <td className="date" title={formatDateTime(change.at)}>
        {formatTime(change.at)}
      </td>
      <td className="col-person">
        <div className="name">
          {change.name}
          <span className="tag kind">{text.kinds[change.entity]}</span>
        </div>
        <div className="sub uid">
          {change.entity === "staff" ? profileText.id : profileText.uid} {change.uid}
        </div>
      </td>
      <td>{label}</td>
      <td className="change-value" title={storedNote}>
        {value}
        {change.moved?.loan && <div className="sub">{text.onLoan}</div>}
        {change.moved?.with !== undefined && <div className="sub">{text.swappedWith(change.moved.with)}</div>}
        {change.moved?.released !== undefined && <div className="sub">{text.released(change.moved.released)}</div>}
        {change.moved?.fee !== undefined && <div className="sub">{text.fee(formatMoney(change.moved.fee))}</div>}
        {change.recall !== undefined && (
          <div className="sub">{text.optionRemoved(formatMoney(change.recall.optionFee))}</div>
        )}
        {captaincyNotes(change).map((note) => (
          <div key={note} className="sub">
            {note}
          </div>
        ))}
      </td>
      <td className="col-status">
        <div className="row-actions">
          {change.undoneAt === null ? (
            <button type="button" disabled={undoing} title={text.undoTitle(label)} onClick={() => onUndo(change)}>
              {text.undo}
            </button>
          ) : (
            text.undone(formatTime(change.undoneAt))
          )}
        </div>
        {change.undoneAt === null && failure !== undefined && (
          <span className="row-error" role="alert">
            {failure}
          </span>
        )}
      </td>
    </tr>
  );
}

/** What undoing a whole mass edit did: how many changes were taken back and which could not be, or why it failed. */
function BatchOutcomeNote({ outcome }: { outcome: BatchOutcome }) {
  if ("error" in outcome) {
    return (
      <span className="row-error" role="alert">
        {outcome.error}
      </span>
    );
  }
  return (
    <div className="row-note" role="status">
      <p>{massEditText.undone(outcome.undone)}</p>
      {outcome.problems.length > 0 && (
        <ul className="edit-problems">
          {outcome.problems.map((problem, index) => (
            <li key={index}>{problem}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
