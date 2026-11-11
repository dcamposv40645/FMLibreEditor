// The state of editing one profile (a player's, a member of staff's or a club's): the inputs, what Apply did,
// and the question asked before changes are lost.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { inputState, summarize, type InputState, type Pending, type Typed } from "./editDraft";
import { applyEdits, readValues } from "./editing";
import { fieldTable } from "./fieldTables";
import { describeError } from "./format";
import { useRefreshHold } from "./refreshHold";
import { editText, fieldLabel } from "./labels";
import type { EditField, Entity, FieldValues, LoggedChange } from "./types";

/** What the app tells the profiles about editing. */
export interface EditorState {
  /**
   * False when nothing can be edited: the system did not let the app write to the game, or the game is a version the
   * editor was not checked with and Edit anyway was not pressed.
   */
  editable: boolean;
  /** Why nothing can be edited, for the hover of what is turned off: the colophon's note. */
  readOnlyNote: string;
  /** Called after changes were written or undone: the lists, the open profile and the change log read again. */
  onEdited: () => void;
}

export const EditorContext = createContext<EditorState>({ editable: false, readOnlyNote: "", onEdited: () => {} });

/** The inputs of the profile being edited (header and body), for the components that draw them. */
export interface EditSession {
  entity: Entity;
  /** Whether the field with this key can be edited here. */
  has: (key: string) => boolean;
  input: (key: string) => InputState | null;
  type: (key: string, text: string) => void;
  /** True while the changes are written: the inputs wait. */
  busy: boolean;
}

/** The inputs of the profile around the reader; null while it is not being edited. */
export const SessionContext = createContext<EditSession | null>(null);

export const useEditSession = () => useContext(SessionContext);

/** Whether the field `field`, if there is one, is an input at the moment. */
export function useEditableField(field: string | undefined): boolean {
  const session = useEditSession();
  return session !== null && field !== undefined && session.has(field);
}

/** View: the profile as it is read. Loading: the field table and the values are being read. Edit: the inputs. */
export type Stage = "view" | "loading" | "edit";

/** What the last Apply did, or why editing could not start. */
export interface Report {
  /** How many changes were written. */
  applied: number;
  /** What went wrong, as lines of text: changes the game refused, and failures of the calls themselves. */
  problems: string[];
}

export interface ProfileEditing {
  /** False when Windows did not allow writing: the profile then has no Edit button. */
  editable: boolean;
  stage: Stage;
  /** The inputs, while editing. */
  session: EditSession | null;
  /** What Apply would write, in the order of the field table. */
  changes: Pending[];
  /** How many inputs hold something that cannot be written. */
  invalid: number;
  /** Whether the list of changes is open. */
  reviewing: boolean;
  /** Whether the footer asks to discard the changes. */
  confirming: boolean;
  applying: boolean;
  report: Report | null;
  /** The Edit button, so that focus can come back to it. */
  editButton: RefObject<HTMLButtonElement | null>;
  /** Reads the field table and the values, then shows the inputs. */
  begin: () => void;
  apply: () => void;
  /** Puts every input back to the value the game has. */
  discard: () => void;
  /** Leaves edit mode, asking first when changes are pending. */
  done: () => void;
  /** The answers to that question. */
  keepEditing: () => void;
  confirmDiscard: () => void;
  toggleReview: () => void;
  dismissReport: () => void;
  /** The close button: closes the profile with `close`, asking first when changes are pending. */
  guardClose: (close: () => void) => void;
  /** Escape: answers the question, else leaves edit mode, else closes the profile with `close`. */
  escape: (close: () => void) => void;
}

/**
 * The profile being edited, if any, is asked before something else takes its place, such as another row or
 * another view. Set while changes are pending: it shows its question and runs `proceed` if they are discarded.
 */
let leaveGuard: ((proceed: () => void) => void) | null = null;

/** Runs `proceed`, unless the profile being edited has changes pending: it asks first. */
export function askToLeave(proceed: () => void): void {
  if (leaveGuard === null) {
    proceed();
  } else {
    leaveGuard(proceed);
  }
}

/** The editing of the player, member of staff or club `uid`, as `entity` says. Mount it with `key={uid}`. */
export function useProfileEditing(entity: Entity, uid: number): ProfileEditing {
  const { editable, onEdited } = useContext(EditorContext);
  const [stage, setStage] = useState<Stage>("view");
  // Nor while a profile is edited: reading the game again would lose what was typed.
  useRefreshHold(stage !== "view");
  const [fields, setFields] = useState<EditField[]>([]);
  // The values the game had when editing began or when the changes were last written, as stored.
  const [start, setStart] = useState<FieldValues>({});
  const [typed, setTyped] = useState<Typed>({});
  // The changes the game refused, with its reasons, until their inputs are typed into again.
  const [refused, setRefused] = useState<Partial<Record<string, string>>>({});
  const [reviewing, setReviewing] = useState(false);
  // Set while the footer asks whether to discard the changes. `then` is what the discarding was for.
  const [confirm, setConfirm] = useState<{ then?: () => void } | null>(null);
  const [applying, setApplying] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const alive = useRef(true);
  const editButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const editing = stage === "edit";
  const byKey = useMemo(() => new Map(fields.map((field) => [field.key, field] as const)), [fields]);
  const { changes, invalid } = useMemo(() => summarize(fields, start, typed), [fields, start, typed]);

  const type = useCallback((key: string, text: string) => {
    setTyped((old) => ({ ...old, [key]: text }));
    setRefused((old) => (old[key] === undefined ? old : without(old, key)));
  }, []);

  const session = useMemo<EditSession | null>(() => {
    if (!editing) {
      return null;
    }
    return {
      entity,
      busy: applying,
      has: (key) => byKey.has(key) && start[key] !== undefined,
      input: (key) => {
        const field = byKey.get(key);
        const stored = start[key];
        return field === undefined || stored === undefined ? null : inputState(field, stored, typed[key], refused[key]);
      },
      type,
    };
  }, [editing, entity, applying, byKey, start, typed, refused, type]);

  // While changes are pending, picking another row or view asks first (see `askToLeave`).
  const guarded = editing && (changes.length > 0 || applying);
  useEffect(() => {
    if (!guarded) {
      return;
    }
    const guard = (proceed: () => void) => {
      if (!applying) {
        setConfirm({ then: proceed });
      }
    };
    leaveGuard = guard;
    return () => {
      if (leaveGuard === guard) {
        leaveGuard = null;
      }
    };
  }, [guarded, applying]);

  /** Leaves edit mode and forgets what was typed. `keepReport` keeps what the last Apply said. */
  const stop = (keepReport: boolean, focus = true) => {
    setStage("view");
    setTyped({});
    setRefused({});
    setReviewing(false);
    setConfirm(null);
    if (!keepReport) {
      setReport(null);
    }
    if (focus) {
      editButton.current?.focus();
    }
  };

  const begin = async () => {
    if (!editable || stage !== "view") {
      return;
    }
    setStage("loading");
    setReport(null);
    try {
      const [table, values] = await Promise.all([fieldTable(entity), readValues(entity, uid)]);
      if (!alive.current) {
        return;
      }
      setFields(table);
      setStart(values);
      setTyped({});
      setRefused({});
      setReviewing(false);
      setConfirm(null);
      setStage("edit");
    } catch (e) {
      if (alive.current) {
        setStage("view");
        setReport({ applied: 0, problems: [editText.startFailed(describeError(e))] });
      }
    }
  };

  // Where keyboard focus goes when an Apply that left the inputs open is over: the buttons are disabled while it
  // runs, and focus on one of them would be lost.
  const refocus = useRef<string | null>(null);
  useEffect(() => {
    if (!applying && refocus.current !== null) {
      editButton.current?.closest(".profile")?.querySelector<HTMLElement>(refocus.current)?.focus();
      refocus.current = null;
    }
  }, [applying]);

  const apply = async () => {
    if (!editing || applying || changes.length === 0 || invalid.length > 0) {
      return;
    }
    setApplying(true);
    setReport(null);
    setConfirm(null);
    try {
      const result = await applyEdits(
        entity,
        uid,
        changes.map((change) => change.edit),
      );
      // The game has changed: the lists, the profile and the change log read it again, whatever happens next.
      if (result.applied.length > 0) {
        onEdited();
      }
      const problems = result.failed.map((failed) =>
        editText.refused(fieldLabel(entity, failed.field), describeError(failed.error)),
      );
      let values: FieldValues | null = null;
      try {
        values = await readValues(entity, uid);
      } catch (e) {
        problems.push(editText.rereadFailed(describeError(e)));
      }
      if (!alive.current) {
        return;
      }
      const refusals = Object.fromEntries(
        result.failed.map((failed) => [failed.field, describeError(failed.error)] as const),
      );
      const refusedKeys = Object.keys(refusals);
      // The inputs show the game's values again; the ones the game refused keep what was typed.
      setStart((old) => values ?? withWritten(old, result.applied));
      setTyped((old) => only(old, refusedKeys));
      setRefused(refusals);
      setReport({ applied: result.applied.length, problems });
      if (refusedKeys.length === 0) {
        stop(true);
      } else {
        refocus.current = `[data-field="${refusedKeys[0]}"]`;
      }
    } catch (e) {
      if (alive.current) {
        setReport({ applied: 0, problems: [describeError(e)] });
        refocus.current = ".edit-footer .primary";
      }
    } finally {
      setApplying(false);
    }
  };

  const discard = () => {
    setTyped({});
    setRefused({});
    setReport(null);
    setConfirm(null);
  };

  const done = () => {
    if (applying) {
      return;
    }
    if (changes.length > 0) {
      setConfirm({});
    } else {
      stop(false);
    }
  };

  const confirmDiscard = () => {
    const then = confirm?.then;
    stop(false, then === undefined);
    then?.();
  };

  const guardClose = (close: () => void) => {
    if (applying) {
      return;
    }
    if (editing && changes.length > 0) {
      setConfirm({ then: close });
    } else {
      close();
    }
  };

  const escape = (close: () => void) => {
    if (applying) {
      return;
    }
    if (confirm !== null) {
      setConfirm(null);
    } else if (editing) {
      done();
    } else {
      close();
    }
  };

  return {
    editable,
    stage,
    session,
    changes,
    invalid: invalid.length,
    reviewing,
    confirming: confirm !== null,
    applying,
    report,
    editButton,
    begin,
    apply,
    discard,
    done,
    keepEditing: () => setConfirm(null),
    confirmDiscard,
    toggleReview: () => setReviewing((open) => !open),
    dismissReport: () => setReport(null),
    guardClose,
    escape,
  };
}

/** `record` without `key`. */
function without<T>(record: Partial<Record<string, T>>, key: string): Partial<Record<string, T>> {
  const rest = { ...record };
  delete rest[key];
  return rest;
}

/** The text typed for `keys`, and nothing else. */
function only(typed: Typed, keys: readonly string[]): Typed {
  const kept: Typed = {};
  for (const key of keys) {
    const text = typed[key];
    if (text !== undefined) {
      kept[key] = text;
    }
  }
  return kept;
}

/** `values` with the values that were just written on top. */
function withWritten(values: FieldValues, applied: readonly LoggedChange[]): FieldValues {
  return { ...values, ...Object.fromEntries(applied.map((change) => [change.field, change.new] as const)) };
}
