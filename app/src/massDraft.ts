// What the inputs of the mass-edit dialog hold and what they stand for. No React here: the dialog only draws it.

import { valueText } from "./editDraft";
import { isWholePercent, numberFromIso, shownRange, storedValue } from "./editing";
import { describeError } from "./format";
import {
  editText,
  fieldLabel,
  massEditText,
  type AttributeKey,
  type StaffAttributeKey,
} from "./labels";
import type { BatchUndo, EditField, Entity, LoggedChange, MassOperation, MassOperationKind } from "./types";

/** Most fields one mass edit changes at once. */
export const MAX_OPERATIONS = 5;

/** Most players, staff or clubs one mass edit takes: the limit of `plan_mass_edit`, which refuses more. */
export const MASS_LIMIT = 10_000;

/** What an operation does: the `op` of a `MassOperationKind`, which is also what the operation select holds. */
export type OperationKind = MassOperationKind["op"];

/** The values a Scale takes, as typed: a whole percent of the value the game has now. */
export const PERCENT_RANGE = { min: 1, max: 1000 };

/** One row of the dialog, as typed. */
export interface OperationDraft {
  /** Tells the rows apart across renders; it is not sent anywhere. */
  id: number;
  /** The key of the field to change. */
  field: string;
  kind: OperationKind;
  /** The value input as typed: a number, or `YYYY-MM-DD` for the date of a Set; empty until something is typed. */
  text: string;
}

/** How the inputs of a row stand: what to send, or why not. */
export type RowVerdict =
  | { ok: true; operation: MassOperation }
  /** `message` is why the input cannot be applied; null while it is still empty. */
  | { ok: false; message: string | null };

/** The whole number `text` holds; null when it is empty or anything else. */
function wholeNumber(text: string): number | null {
  const trimmed = text.trim();
  const value = Number(trimmed);
  // Adding zero turns a typed "-0" into 0.
  return trimmed !== "" && Number.isInteger(value) ? value + 0 : null;
}

/**
 * The amounts an Add takes, as typed (in the units the field is shown in): as many as the field's range is wide,
 * either way. A larger one would only push every value to the end of its range.
 */
export function addRange(field: EditField): { min: number; max: number } {
  const { min, max } = shownRange(field);
  return { min: min - max, max: max - min };
}

/** The numbers the value input of `field` takes for `kind`, as typed (not for the date of a Set). */
export function inputRange(field: EditField, kind: OperationKind): { min: number; max: number } {
  switch (kind) {
    case "set":
      return shownRange(field);
    case "add":
      return addRange(field);
    case "scale":
      return PERCENT_RANGE;
  }
}

/**
 * What the row that does `kind` to `field` and holds `text` stands for. Values are sent as stored: a Set of an
 * attribute is typed on the 1–20 scale it is shown on and written on the 0–100 scale it is stored on, and an Add of
 * N points is N times as much; a Set of a percentage is typed as 0–100 and written as 0–10000, and an Add takes
 * percentage points, 100 times as many; a Scale is a percent of the stored value, whatever the field.
 */
export function judgeRow(field: EditField, kind: OperationKind, text: string): RowVerdict {
  const empty = text.trim() === "";
  const ok = (operation: MassOperationKind): RowVerdict => ({ ok: true, operation: { field: field.key, operation } });
  // An empty input is not marked as wrong, only unusable.
  const invalid = (message: string): RowVerdict => ({ ok: false, message: empty ? null : message });

  if (field.kind === "date" && kind !== "set") {
    return invalid(massEditText.dateSetOnly);
  }
  switch (kind) {
    case "set": {
      if (field.kind === "date") {
        const value = numberFromIso(text);
        return value !== null && value >= field.min && value <= field.max
          ? ok({ op: "set", value })
          : invalid(editText.invalidDate(valueText(field, field.min), valueText(field, field.max)));
      }
      const shown = wholeNumber(text);
      const { min, max } = shownRange(field);
      if (shown !== null && shown >= min && shown <= max) {
        return ok({ op: "set", value: storedValue(field, shown) });
      }
      return invalid(
        field.kind === "i32"
          ? editText.invalidMoney(valueText(field, min), valueText(field, max))
          : field.scale === "percent" || isWholePercent(field)
            ? editText.invalidPercentage(valueText(field, min), valueText(field, max))
            : editText.invalid(min, max),
      );
    }
    case "add": {
      const amount = wholeNumber(text);
      const { min, max } = addRange(field);
      return amount !== null && amount >= min && amount <= max
        ? ok({ op: "add", amount: storedValue(field, amount) })
        : invalid(massEditText.invalidAdd(min, max));
    }
    case "scale": {
      const percent = wholeNumber(text);
      return percent !== null && percent >= PERCENT_RANGE.min && percent <= PERCENT_RANGE.max
        ? ok({ op: "scale", percent })
        : invalid(massEditText.invalidPercent(PERCENT_RANGE.min, PERCENT_RANGE.max));
    }
  }
}

// The field select ---------------------------------------------------------------------------------------------------

/** A group of the field select; its name is in `massEditText.groups`. */
export type FieldGroupId = keyof typeof massEditText.groups;

/** The groups of each list's field select, in order. A field that belongs to none of them is in "other". */
const GROUP_ORDER: Record<Entity, readonly FieldGroupId[]> = {
  player: [
    "ability", "technical", "mental", "physical", "goalkeeping", "hidden", "feet", "positions", "personality",
    "reputation", "international", "contract", "health", "traits", "other",
  ],
  staff: ["ability", "coaching", "mental", "knowledge", "medical", "personality", "reputation", "contract", "other"],
  club: ["reputation", "facilities", "finances", "stadium", "other"],
};

/**
 * The attributes of each group of the player select. Unlike the profile, which shows first touch and passing under
 * Goalkeeping too, every attribute is in one group only: it is one entry of the select.
 */
const PLAYER_ATTRIBUTE_GROUPS: Partial<Record<FieldGroupId, readonly AttributeKey[]>> = {
  technical: [
    "corners", "crossing", "dribbling", "finishing", "first_touch", "free_kicks", "heading", "long_shots",
    "long_throws", "marking", "passing", "penalty_taking", "tackling", "technique",
  ],
  mental: [
    "aggression", "anticipation", "bravery", "composure", "concentration", "decisions", "determination", "flair",
    "leadership", "off_the_ball", "positioning", "teamwork", "vision", "work_rate",
  ],
  physical: ["acceleration", "agility", "balance", "jumping_reach", "natural_fitness", "pace", "stamina", "strength"],
  goalkeeping: [
    "aerial_reach", "command_of_area", "communication", "eccentricity", "handling", "kicking", "one_on_ones",
    "punching", "reflexes", "rushing_out", "throwing",
  ],
  hidden: ["consistency", "dirtiness", "important_matches", "injury_proneness", "versatility"],
  feet: ["left_foot", "right_foot"],
};

/** The same for the attributes of staff. */
const STAFF_ATTRIBUTE_GROUPS: Partial<Record<FieldGroupId, readonly StaffAttributeKey[]>> = {
  coaching: [
    "attacking", "defending", "fitness", "goalkeeping", "possession", "set_pieces", "tactical", "technical",
    "working_with_youngsters",
  ],
  mental: ["authority", "determination", "motivating", "people_management"],
  knowledge: [
    "data_analysis", "judging_ability", "judging_potential", "judging_staff", "negotiating", "tactical_knowledge",
  ],
  medical: ["physiotherapy", "sports_science"],
};

/** The group of the attribute `name` of players or of staff; none for a name no group has. */
function attributeGroup(entity: Entity, name: string): FieldGroupId | undefined {
  const groups = entity === "staff" ? STAFF_ATTRIBUTE_GROUPS : PLAYER_ATTRIBUTE_GROUPS;
  return (Object.keys(groups) as FieldGroupId[]).find((id) => (groups[id] as readonly string[]).includes(name));
}

/** The group of the field `key` in the select of `entity`; "other" for one the groups do not know. */
function groupOf(entity: Entity, key: string): FieldGroupId {
  const [prefix, name = ""] = key.split(".");
  let id: FieldGroupId | undefined;
  switch (prefix) {
    case "current_ability":
    case "potential_ability":
      id = "ability";
      break;
    case "attribute":
      id = attributeGroup(entity, name);
      break;
    case "position":
      id = "positions";
      break;
    case "personality":
    case "reputation":
    case "international":
    case "contract":
    case "facilities":
      id = prefix;
      break;
    case "finance":
      id = "finances";
      break;
    case "stadium":
      id = "stadium";
      break;
    case "trait":
      id = "traits";
      break;
    // What a player's Medical group in his profile has: his energy, match sharpness, jadedness and morale.
    case "condition":
    case "match_sharpness":
    case "jadedness":
    case "morale":
      id = "health";
      break;
  }
  return id !== undefined && GROUP_ORDER[entity].includes(id) ? id : "other";
}

/** One group of the field select and its fields, in the order of the field table. */
export interface FieldGroup {
  id: FieldGroupId;
  fields: EditField[];
}

/** `fields` sorted into the groups of the select of `entity`, in their order; a group without fields is left out. */
export function groupFields(entity: Entity, fields: readonly EditField[]): FieldGroup[] {
  const byGroup = new Map<FieldGroupId, EditField[]>();
  for (const field of fields) {
    const id = groupOf(entity, field.key);
    const group = byGroup.get(id);
    if (group === undefined) {
      byGroup.set(id, [field]);
    } else {
      group.push(field);
    }
  }
  return GROUP_ORDER[entity].flatMap((id) => {
    const group = byGroup.get(id);
    return group === undefined ? [] : [{ id, fields: group }];
  });
}

// Undo ---------------------------------------------------------------------------------------------------------------

/**
 * What to tell about the changes a batch undo could not take back: for each, who and which field when `log` has the
 * change, otherwise its number; then the game's reason.
 */
export function undoFailureLines(failed: BatchUndo["failed"], log: readonly LoggedChange[]): string[] {
  const byId = new Map(log.map((change) => [change.id, change] as const));
  return failed.map(({ id, error }) => {
    const change = byId.get(id);
    const who = change ? `${change.name} · ${fieldLabel(change.entity, change.field)}` : massEditText.changeNumber(id);
    return massEditText.undoFailure(who, describeError(error));
  });
}
