// Text of the player, staff and club profiles and lists, as the components import it. The wording is in i18n/en.ts,
// i18n/tr.ts and i18n/es.ts (one shape, one file per language); this file hands out the language chosen at startup (see
// i18n/index.ts) under the names the components use. Keys of the game data map to the names FM uses; every other
// string of the panels is in `profileText` (those only the club panel has, in `clubProfileText`), and those of the
// player, staff and club lists in `playerListText`, `staffListText` and `clubListText`. Editing has `editText` (and
// `fieldLabel` for the names of the fields), the Changes view `changesText`, mass editing `massEditText`, comparing
// players `comparisonText`, moving a player to another club (or swapping him with one) `moveText`, releasing him
// `releaseText`, his nationalities and homegrown status `tieText`, a club's squad `squadText`, the sidebar and the connect panel `appText`, the status of the
// graphics packs `graphicsText`, and new versions of the app `updateText`.
// A new language only has to provide a dictionary of the same shape (and be added to i18n/index.ts, with a flag in
// SettingsMenu.tsx).

import { formatNumber } from "./format";
import { intlLocale, strings } from "./i18n";
import type { Strings } from "./i18n/en";
import type { CaptainRole, Entity, Nation } from "./types";

export const ATTRIBUTE_LABELS = strings.attributes;

export type AttributeKey = keyof Strings["attributes"];

export const STAFF_ATTRIBUTE_LABELS = strings.staffAttributes;

export type StaffAttributeKey = keyof Strings["staffAttributes"];

export const PERSONALITY_LABELS = strings.personality;

export type PersonalityKey = keyof Strings["personality"];

export const POSITION_LABELS = strings.positions;

export type PositionKey = keyof Strings["positions"];

/** The abbreviation of a position by its key ("st" is "ST"); a key the table lacks is shown upper-cased. */
export function positionLabel(key: string): string {
  return labelIn(POSITION_LABELS, key) ?? key.toUpperCase();
}

export const CLUB_FACILITY_LABELS = strings.clubFacilities;

export const SUPPORTER_TRAIT_LABELS = strings.supporterTraits;

/** The kinds of supporters, in the order of the supporter-mix bar. */
export const SUPPORTER_MIX_LABELS = strings.supporterMix;

export type SupporterMixKey = keyof Strings["supporterMix"];

/** FM's job codes as stored in contracts, with their names. */
const JOB_LABELS: Partial<Record<number, string>> = strings.jobs;

export const profileText = strings.profile;

/**
 * The name of a job code from a contract. An odd code above 1 is the player-coach variant of the
 * code below it. A person without a contract (`null`) has no job; codes the table does not know
 * are shown as they are.
 */
export function jobLabel(code: number | null): string {
  if (code === null) {
    return profileText.none;
  }
  const label = JOB_LABELS[code];
  if (label !== undefined) {
    return label;
  }
  const coach = code > 1 && code % 2 === 1 ? JOB_LABELS[code - 1] : undefined;
  return coach !== undefined ? profileText.playerCoach(coach) : profileText.unknownJob(code);
}

/** The names of the nations a nation field can be set to, by address, once they have been read (see `fieldTable`). */
const NATION_NAMES = new Map<number, string>();

/** Keeps the names of `nations`, for the nation field's select and for the change log. */
export function nameNations(nations: readonly Nation[]): void {
  for (const nation of nations) {
    NATION_NAMES.set(nation.address, nation.name);
  }
}

/** How the codes of a field that holds codes from a list (see `EditField.choices`) are named, by the field's key. */
const CHOICE_NAMES = new Map<string, (code: number) => string>([
  ["contract.squad_status", profileText.squadStatusName],
  ["nation", (address) => NATION_NAMES.get(address) ?? editText.unknownNation],
]);

/**
 * The name of `code`, a value of the field `key` that holds codes from a list; a field whose codes have no names
 * shows them as numbers. A code the game holds that the field does not offer is named the same way.
 */
export function choiceLabel(key: string, code: number): string {
  return CHOICE_NAMES.get(key)?.(code) ?? formatNumber(code);
}

/**
 * The jobs the staff list can be filtered by, one entry each, in the order of their names (as the language
 * sorts them). The odd codes (player-coaches) are left out: a filter on a coaching job finds them too. So is
 * "Player" (code 1), which is no job of a member of staff and, with the next code added, would match the coaches.
 */
export const JOB_FILTER_OPTIONS: readonly { code: number; label: string }[] = Object.keys(JOB_LABELS)
  .map(Number)
  .filter((code) => code > 1)
  .map((code) => ({ code, label: jobLabel(code) }))
  .sort((a, b) => a.label.localeCompare(b.label, intlLocale));

/** The player list: search, filter bar and columns. */
export const playerListText = strings.playerList;

/** The staff list. */
export const staffListText = strings.staffList;

/** The club profile: what the player and staff panels do not have. */
export const clubProfileText = strings.clubProfile;

/** The club list, with the sidebar entry and the status-line count that go with it. */
export const clubListText = strings.clubList;

/**
 * The app around the lists: the sidebar (and the note at its foot, which depends on the connection, see
 * `ConnectionInfo.editable`), the status line and the connect panel.
 */
export const appText = strings.app;

/** The graphics packs the pictures come from: their status at the foot of the sidebar. */
export const graphicsText = strings.graphics;

/** New versions: the notice under the masthead and Check for updates at the foot of the window. */
export const updateText = strings.update;

/** A version of the game the editor was not checked with, or whose data did not check out: the notice about it. */
export const compatibilityText = strings.compatibility;

/** What each of the checks the editor makes before it writes is about, as the notice and the errors name them. */
export const CHECK_NAMES = strings.checks;

/** Editing a person or a club: the Edit button, the inputs, and the footer under the profile. */
export const editText = strings.edit;

/** The Changes view: what was written to the game this session. */
export const changesText = strings.changes;

/** Mass editing: the button in the toolbar of a list and the dialog it opens. */
export const massEditText = strings.massEdit;

/** Exporting players to a file: the button and the dialog. */
export const exportText = strings.export;

/** Comparing players: the button in a profile, the tray under the player list and the dialog. */
export const comparisonText = strings.comparison;

/** Moving a player to another club, or swapping him with one: the button in his profile and the dialog it opens. */
export const moveText = strings.move;

/** Releasing a player from his club: the button beside Transfer in his profile and the dialog it opens. */
export const releaseText = strings.release;

/** Recalling a player from his loan: the dialog the Recall button in his profile opens. */
export const recallText = strings.recall;

/** Making a player's loan permanent: the dialog the Make permanent button in his profile opens. */
export const permanentText = strings.permanent;

/** Sending a player on loan: the button beside Transfer, and its dialog (see LoanDialog.tsx). */
export const loanStartText = strings.loanStart;

/** Choosing the overlay's shortcut (see ShortcutDialog.tsx). */
export const shortcutText = strings.shortcut;

/** A player's secondary nationalities and homegrown status: the group in his profile and its dialog. */
export const tieText = strings.ties;

/** A contract's bonuses and clauses: the button under them in the profile and its dialog. */
export const clauseText = strings.clauses;

/** A club's squad: the tab of its profile with its teams' players, the actions on a team and the captains. */
export const squadText = strings.squad;

/** The kind of team the game calls a B team (see `ClubTeam.kind`). */
const B_TEAM_KIND = 3;

/** What names a team: its place in the club's list, and where it is known the kind and age limit the game gives it. */
export interface TeamId {
  index: number;
  kind?: number;
  ageLimit?: number | null;
}

/**
 * The name of a club's team as its squad screen shows it: the first team, a team by its age limit ("U21"), a B team,
 * and any other by its place in the club's list ("Team 3"). Where only the place is known, as in the Changes view, a
 * team other than the first is named by it alone.
 */
export function teamLabel({ index, kind, ageLimit }: TeamId): string {
  if (index === 0) {
    return squadText.teams.first;
  }
  if (ageLimit !== undefined && ageLimit !== null) {
    return squadText.teams.age(ageLimit);
  }
  return kind === B_TEAM_KIND ? squadText.teams.b : squadText.teams.other(index + 1);
}

/**
 * What a captaincy that swapped the two captains says about the other player: `from` held `role`, which the new holder
 * has taken, and now holds the role the new holder left. When nobody held `role` (`from` is null) that one is empty now.
 */
export function swapNote(role: CaptainRole, from: string | null): string {
  if (role === "captain") {
    return from === null ? squadText.noViceCaptain : squadText.nowViceCaptain(from);
  }
  return from === null ? squadText.noCaptain : squadText.nowCaptain(from);
}

const asText = (value: number | string) => (typeof value === "number" ? formatNumber(value) : value);

/**
 * "17 → 20": a value before and after a change, as shown. A number gets the reader's thousands separators; text,
 * such as a date or an amount of money already written out (see `valueText` in editDraft.ts), is used as it is.
 */
export const valueChange = (from: number | string, to: number | string) => `${asText(from)} → ${asText(to)}`;

/** The text of `table` for `key`; nothing for a key the table lacks, however it is spelled (e.g. "constructor"). */
function labelIn(table: object, key: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(table, key) ? (table as Record<string, string>)[key] : undefined;
}

/** The names of the fields that are not attributes, personality or positions, by key. */
const FIELD_LABELS: Readonly<Record<string, string>> = strings.fields;

/**
 * The name of an editable field, from the key `edit_fields` gives it ("current_ability", "attribute.finishing",
 * "position.st", "contract.wage", "finance.balance", …). A key it does not know is returned as it is.
 */
export function fieldLabel(entity: Entity, key: string): string {
  const fixed = labelIn(FIELD_LABELS, key);
  if (fixed !== undefined) {
    return fixed;
  }
  const [group, name = ""] = key.split(".");
  switch (group) {
    case "attribute":
      return labelIn(entity === "staff" ? STAFF_ATTRIBUTE_LABELS : ATTRIBUTE_LABELS, name) ?? key;
    case "personality":
      return labelIn(PERSONALITY_LABELS, name) ?? key;
    case "trait":
      return profileText.traitName(Number(name));
    case "position": {
      const position = labelIn(POSITION_LABELS, name);
      return position === undefined ? key : editText.positionRating(position);
    }
    default:
      return key;
  }
}

export const scoutingText = strings.scouting;
