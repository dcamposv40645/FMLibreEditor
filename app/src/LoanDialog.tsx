// Sending a player on loan: the button beside Transfer in his profile, and the dialog it opens. The dialog takes the club
// he goes to (any club, from a search), the loan's last day and the wage that club pays (his own by default), and what
// the clubs agree: whether his club may recall him, whether the loan may be ended early, and where he may play. It asks
// the game whether the loan can be made before it offers to make it, and makes it as the game makes its own (see
// `plan_loan` in crates/fmlibre-core/src/loans.rs). It is logged like a move between the two clubs, and one undo from
// the Changes view takes it back; left alone, the game ends it on its last day and he is back at his club.

import { useContext, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { call } from "./api";
import { ClubSearch } from "./ClubSearch";
import { toPounds, toShown } from "./currency";
import { dateFromNumber, isoFromNumber, numberFromDate, planLoan, startLoan } from "./editing";
import { currencySymbol, describeError, formatDate, formatMoney } from "./format";
import { loanStartText as text, moveText } from "./labels";
import { ClubLine } from "./MoveDialog";
import { inputClass, judgeExpires, judgeWage, PLAN_DELAY, termNote } from "./moveTerms";
import type { ClubSummary, ConnectionInfo, Contract, FmDate, MovePreview, NewLoan } from "./types";
import { useModal } from "./useModal";
import { EditorContext } from "./useProfileEditing";

interface LoanPlayer {
  uid: number;
  name: string;
  club: string | null;
  clubUid: number | null;
}

/**
 * Send on loan, for a player with a club who is not on loan already; it waits for his contract, whose wage and end the
 * dialog starts from.
 */
export function LoanButton({ player, contract }: { player: LoanPlayer; contract: Contract | null }) {
  const { onEdited } = useContext(EditorContext);
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);

  // Once the dialog is gone, focus goes back to the button that opened it.
  useEffect(() => {
    if (wasOpen.current && !open) {
      button.current?.focus();
    }
    wasOpen.current = open;
  }, [open]);

  return (
    <>
      <button
        ref={button}
        type="button"
        className="move-toggle"
        aria-haspopup="dialog"
        disabled={contract === null}
        title={text.buttonTitle}
        onClick={() => setOpen(true)}
      >
        {text.button}
      </button>
      {open && contract !== null && (
        <LoanDialog player={player} contract={contract} onDone={onEdited} onClose={() => setOpen(false)} />
      )}
    </>
  );
}

interface LoanDialogProps {
  player: LoanPlayer;
  /** His own contract: the loan's wage starts from its wage, and the loan may not run past its end. */
  contract: Contract;
  /** Called once he is on loan: the game has changed, so the lists and the profile read it again. */
  onDone: () => void;
  onClose: () => void;
}

/**
 * The loan's last day when none is picked, as a number like 20330630: the end of the season under way in the game
 * (30 June), or of his contract when that comes first. As `plan_loan` takes it.
 */
function defaultUntil(today: FmDate, contractEnd: number | null): number {
  const seasonEnd = (today.month <= 6 ? today.year : today.year + 1) * 10_000 + 630;
  return contractEnd === null ? seasonEnd : Math.min(seasonEnd, contractEnd);
}

/** The day after `date`, as YYYY-MM-DD: the first a loan can end on. */
function dayAfter(date: FmDate): string {
  const next = new Date(Date.UTC(date.year, date.month - 1, date.day + 1));
  return next.toISOString().slice(0, 10);
}

/** What the game said to the plan, for the club and terms it was asked about. */
interface Planned {
  key: string;
  outcome: { preview: MovePreview } | { error: string };
}

const planKey = (club: number, loan: NewLoan) =>
  [club, loan.until ?? "", loan.wage ?? "", loan.canBeRecalled, loan.canBeTerminated, loan.cupMatches, loan.againstOwnClub].join(":");

export function LoanDialog({ player, contract, onDone, onClose }: LoanDialogProps) {
  const base = useId();
  const id = (part: string) => `${base}-${part}`;
  const dialog = useRef<HTMLDivElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const alive = useRef(true);
  const running = useRef(false);

  const [club, setClub] = useState<ClubSummary | null>(null);
  // The wage as his contract has it, in pounds; the input holds it in the chosen currency.
  const wasWageShown = toShown(contract.wage);
  const [wageText, setWageText] = useState(String(wasWageShown));
  // The game's date, which the loan's last day starts from and must come after; the box is filled in once it is read.
  const [today, setToday] = useState<FmDate | null>(null);
  const contractEnd = contract.expires ? numberFromDate(contract.expires) : null;
  const fallback = today === null ? null : defaultUntil(today, contractEnd);
  const [untilText, setUntilText] = useState("");
  const [canBeRecalled, setCanBeRecalled] = useState(true);
  const [canBeTerminated, setCanBeTerminated] = useState(true);
  const [cupMatches, setCupMatches] = useState(true);
  const [againstOwnClub, setAgainstOwnClub] = useState(false);
  const [planned, setPlanned] = useState<Planned | null>(null);
  const [making, setMaking] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useModal(dialog, !making, onClose);

  useEffect(() => {
    alive.current = true;
    // Without it the date box would show the computer's date when empty: it shows the game's default instead.
    call<ConnectionInfo | null>("current_connection").then(
      (info) => {
        if (alive.current && info?.gameDate) {
          const date = info.gameDate;
          setToday(date);
          setUntilText((text) => (text === "" ? isoFromNumber(defaultUntil(date, contractEnd)) : text));
        }
      },
      () => {},
    );
    return () => {
      alive.current = false;
    };
    // Read once, when the dialog opens.
  }, []);

  const wageVerdict = judgeWage(wageText, wasWageShown);
  // The default is no change: the game works it out again as it writes.
  const untilVerdict = judgeExpires(untilText, untilText === "" ? null : fallback);
  const valid = wageVerdict.status !== "invalid" && untilVerdict.status !== "invalid";
  const loan: NewLoan = {
    until: untilVerdict.status === "changed" ? untilVerdict.value : null,
    wage: wageVerdict.status === "changed" ? toPounds(wageVerdict.value) : null,
    canBeRecalled,
    canBeTerminated,
    cupMatches,
    againstOwnClub,
  };
  const key = club === null ? null : planKey(club.uid, loan);

  // The plan, asked a moment after the last change of the club or the terms; an answer for older ones is dropped.
  useEffect(() => {
    if (club === null || !valid) {
      return;
    }
    let cancelled = false;
    const asked = loan;
    const askedKey = planKey(club.uid, asked);
    const timer = setTimeout(() => {
      planLoan(player.uid, club.uid, asked).then(
        (preview) => !cancelled && setPlanned({ key: askedKey, outcome: { preview } }),
        (e) => !cancelled && setPlanned({ key: askedKey, outcome: { error: describeError(e) } }),
      );
    }, PLAN_DELAY);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
    // `loan` is rebuilt each render; its parts, which `key` holds, are what matter.
  }, [player.uid, key, valid]);

  const outcome = planned !== null && planned.key === key ? planned.outcome : null;
  const canMake = club !== null && valid && outcome !== null && "preview" in outcome && !making;

  const make = async () => {
    if (!canMake || running.current || club === null) {
      return;
    }
    running.current = true;
    setMaking(true);
    setFailure(null);
    try {
      await startLoan(player.uid, club.uid, loan);
      onDone();
      onClose();
    } catch (e) {
      if (alive.current) {
        setFailure(describeError(e));
        // The game may not be as the plan found it: the plan is asked again before it is offered again.
        setPlanned(null);
      }
    } finally {
      running.current = false;
      if (alive.current) {
        setMaking(false);
      }
    }
  };

  // Where he goes, until when and on what wage; or why he cannot. A live region always there.
  const route = (made: MovePreview) => {
    const lines = [moveText.route(made.from?.name ?? player.club ?? "", made.to.name)];
    if (made.contract) {
      const until = dateFromNumber(made.contract.expires);
      lines.push(text.terms(until ? formatDate(until) : String(made.contract.expires), formatMoney(made.contract.wage)));
    }
    return lines.join("\n");
  };
  const plan: { tone: "pending" | "ok" | "refused"; message: string } =
    club === null
      ? { tone: "pending", message: text.pickClub }
      : !valid
        ? { tone: "pending", message: moveText.fixTerms }
        : outcome === null
          ? { tone: "pending", message: text.checking }
          : "error" in outcome
            ? { tone: "refused", message: outcome.error }
            : { tone: "ok", message: route(outcome.preview) };

  const wageNote = termNote(wageVerdict, moveText.wageInvalid, formatMoney(contract.wage));
  const contractEndShown = contract.expires ? formatDate(contract.expires) : null;
  const untilNote =
    untilVerdict.status === "invalid"
      ? { message: moveText.expiresInvalid, invalid: true }
      : { message: text.untilHint(contractEndShown), invalid: false };
  const latest = contractEnd === null ? undefined : isoFromNumber(contractEnd);
  const earliest = today === null ? undefined : dayAfter(today);

  const check = (label: string, on: boolean, set: (on: boolean) => void) => (
    <label className="loan-check">
      <input type="checkbox" checked={on} disabled={making} onChange={(e) => set(e.currentTarget.checked)} />
      {label}
    </label>
  );

  return createPortal(
    // A click beside the dialog does nothing, and keeps focus where it is.
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
        className="modal move-dialog loan-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={id("title")}
        aria-describedby={id("player")}
        aria-busy={making || undefined}
        tabIndex={-1}
      >
        <header className="modal-head">
          <h2 id={id("title")}>{text.title}</h2>
          <p id={id("player")} className="move-player">
            {player.name}
          </p>
        </header>

        <div className="modal-body">
          {player.club !== null && (
            <section className="move-section" aria-labelledby={id("from")}>
              <h3 id={id("from")}>{text.from}</h3>
              <div className="move-club">
                <ClubLine uid={player.clubUid} name={player.club} />
              </div>
            </section>
          )}

          <section className="move-section" aria-labelledby={id("to")}>
            <h3 id={id("to")}>{text.to}</h3>
            {club === null ? (
              <ClubSearch label={text.to} onPick={setClub} onCancel={onClose} />
            ) : (
              <div className="move-club">
                <ClubLine uid={club.uid} name={club.name} women={club.women} />
                <button type="button" className="move-change" disabled={making} onClick={() => setClub(null)}>
                  {text.pickAnother}
                </button>
              </div>
            )}
          </section>

          <section className="move-section" aria-labelledby={id("terms")}>
            <h3 id={id("terms")}>{text.termsHeading}</h3>
            <div className="move-terms">
              <div className="move-term">
                <label htmlFor={id("until")}>{text.until}</label>
                <input
                  id={id("until")}
                  className={inputClass(untilVerdict)}
                  type="date"
                  min={earliest}
                  max={latest}
                  value={untilText}
                  readOnly={making}
                  aria-invalid={untilVerdict.status === "invalid" || undefined}
                  aria-describedby={id("until-note")}
                  onChange={(e) => setUntilText(e.currentTarget.value)}
                />
                <p id={id("until-note")} className={untilNote.invalid ? "move-note invalid" : "move-note"}>
                  {untilNote.message}
                </p>
              </div>

              <div className="move-term">
                <label htmlFor={id("wage")}>{text.wage}</label>
                <span className="move-money">
                  <span className="move-unit" aria-hidden="true">
                    {currencySymbol}
                  </span>
                  <input
                    id={id("wage")}
                    className={inputClass(wageVerdict)}
                    type="number"
                    min={0}
                    step={1}
                    value={wageText}
                    readOnly={making}
                    aria-invalid={wageVerdict.status === "invalid" || undefined}
                    aria-describedby={wageNote ? id("wage-note") : undefined}
                    onChange={(e) => setWageText(e.currentTarget.value)}
                    onWheel={(e) => e.currentTarget.blur()}
                  />
                </span>
                {wageNote && (
                  <p id={id("wage-note")} className={wageNote.invalid ? "move-note invalid" : "move-note"}>
                    {wageNote.message}
                  </p>
                )}
              </div>
            </div>
            <div className="loan-checks">
              {check(text.canBeRecalled, canBeRecalled, setCanBeRecalled)}
              {check(text.canBeTerminated, canBeTerminated, setCanBeTerminated)}
              {check(text.cupMatches, cupMatches, setCupMatches)}
              {check(text.againstOwnClub, againstOwnClub, setAgainstOwnClub)}
            </div>
          </section>

          <p className={`move-plan ${plan.tone}`} role="status">
            {plan.message}
          </p>

          <section className="move-section" aria-labelledby={id("does")}>
            <h3 id={id("does")}>{text.doesHeading}</h3>
            <ul className="move-facts">
              {text.does.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          </section>
        </div>

        {failure !== null && (
          <div className="banner move-error" role="alert">
            {failure}
          </div>
        )}

        <footer className="modal-foot">
          <span className="move-foot-note">{text.hint}</span>
          <button type="button" className="primary" disabled={!canMake} onClick={() => void make()}>
            {making ? text.confirming : text.confirm}
          </button>
          <button ref={cancelButton} type="button" disabled={making} onClick={onClose}>
            {text.cancel}
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
