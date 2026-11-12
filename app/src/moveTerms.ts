// The terms of a transfer as the dialogs take them: the fee and the new wage and contract end typed in, judged before
// they are sent, and what a fee does to each club's money. Shared by the transfer dialog and the dialog that makes a
// loan permanent.

import { toShown } from "./currency";
import { isoFromNumber, numberFromIso } from "./editing";
import { formatMoney } from "./format";
import { editText, moveText as text } from "./labels";
import type { ClubMoney, MoveTerms } from "./types";

/** How long a dialog waits after the last change of the club or the terms before it asks the game for a plan. */
export const PLAN_DELAY = 200;

/** How the text of an input stands against the value the contract had when the dialog opened. */
export type Verdict = { status: "same" } | { status: "changed"; value: number } | { status: "invalid" };

const SAME: Verdict = { status: "same" };
const INVALID: Verdict = { status: "invalid" };

/** The whole number `text` holds; null when it is empty or anything else (a fraction, words, too big to send). */
function wholeNumber(text: string): number | null {
  const trimmed = text.trim();
  const value = Number(trimmed);
  // Adding zero turns a typed "-0" into 0.
  return trimmed !== "" && Number.isSafeInteger(value) ? value + 0 : null;
}

/**
 * The wage input: a change to send, no change (also when the text is another way of writing what the contract had,
 * or the contract has none and nothing was typed), or unusable. Whether the game takes the number is for the plan
 * to say.
 */
export function judgeWage(text: string, was: number | null): Verdict {
  if (text === (was === null ? "" : String(was))) {
    return SAME;
  }
  const value = wholeNumber(text);
  if (value === null || value < 0) {
    return INVALID;
  }
  return value === was ? SAME : { status: "changed", value };
}

/** The most a fee can be, in pounds: what the game's money fields hold; and as typed, in the chosen currency. */
const MAX_FEE = 2_000_000_000;
export const MAX_FEE_SHOWN = toShown(MAX_FEE);

/**
 * The fee input: no fee while it is empty or 0, a whole amount of the chosen currency up to `MAX_FEE_SHOWN`, or
 * unusable. The value is as typed; the pounds to send are worked out from it.
 */
export function judgeFee(text: string): Verdict {
  if (text.trim() === "") {
    return SAME;
  }
  const value = wholeNumber(text);
  if (value === null || value < 0 || value > MAX_FEE_SHOWN) {
    return INVALID;
  }
  return value === 0 ? SAME : { status: "changed", value };
}

/** The same for the contract end, which a date input holds as `YYYY-MM-DD`: unusable until it is a whole, real date. */
export function judgeExpires(text: string, was: number | null): Verdict {
  if (text === (was === null ? "" : isoFromNumber(was))) {
    return SAME;
  }
  const value = numberFromIso(text);
  return value === null ? INVALID : value === was ? SAME : { status: "changed", value };
}

/** The terms to send: only what was changed, so that everything else stays as the contract has it. */
export function termsOf(wage: number | undefined, expires: number | undefined): MoveTerms {
  return { ...(wage !== undefined && { wage }), ...(expires !== undefined && { expires }) };
}

/** What a fee does to one club's money, a line for the plan: what it pays or gets, and its money before and after. */
export function moneyLine(money: ClubMoney | null, club: string, fee: string, paying: boolean): string {
  if (money === null) {
    return text.noFinances(club);
  }
  const change = ({ before, after }: { before: number; after: number }) =>
    `${formatMoney(before)} → ${formatMoney(after)}`;
  return paying
    ? text.pays(club, fee, change(money.balance), money.transferBudget && change(money.transferBudget))
    : text.receives(club, fee, change(money.balance), money.transferBudget && change(money.transferBudget));
}

/** An input's class: outlined in the accent colour once it holds a change, and as an error while it cannot be sent. */
export const inputClass = (verdict: Verdict) => (verdict.status === "same" ? "move-input" : `move-input ${verdict.status}`);

/** What goes under an input: why it cannot be sent, or the value it started from once it holds another. */
export function termNote(verdict: Verdict, invalid: string, was: string | null): { message: string; invalid: boolean } | null {
  if (verdict.status === "invalid") {
    return { message: invalid, invalid: true };
  }
  return verdict.status === "changed" && was !== null ? { message: editText.was(was), invalid: false } : null;
}
