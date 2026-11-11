import { useContext, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { EditableValue } from "./EditControls";
import { freezeMoney, frozenClubs, renameStadium, stadiumName } from "./editing";
import { converts, inCurrency } from "./currency";
import { describeError, formatMoney, formatNumber } from "./format";
import { InfoTip } from "./InfoTip";
import {
  appText,
  CLUB_FACILITY_LABELS,
  clubProfileText,
  profileText,
  SUPPORTER_MIX_LABELS,
  SUPPORTER_TRAIT_LABELS,
  type SupporterMixKey,
} from "./labels";
import { ClubLogo, PICTURE_SIZE } from "./Picture";
import {
  Fact,
  ProfileFrame,
  RatingGroup,
  RatingList,
  ReputationMeter,
  useProfileDetails,
  type PanelIds,
  type RatingRow,
} from "./ProfileParts";
import { SquadTab } from "./SquadTab";
import type { ClubDetails, ClubSummary, Finances, Stadium, StadiumName } from "./types";
import { EditorContext, useProfileEditing } from "./useProfileEditing";

const text = clubProfileText;

/** The tabs of the profile: what the club has, its squad, and its stadium. */
export type ClubTab = "overview" | "squad" | "stadium";

const TABS: readonly ClubTab[] = ["overview", "squad", "stadium"];

/** The tab saved before the language was switched; the overview when there is none. */
export const readTab = (value: unknown): ClubTab => (value === "squad" || value === "stadium" ? value : "overview");

interface ClubProfileProps {
  /** The row that was picked; its fields fill the header while the full profile loads. */
  club: ClubSummary;
  /** Changes when the game data is read again, so the profile is too. */
  version: number;
  /** The open tab. The list keeps it, so that the next club picked opens on it too. */
  tab: ClubTab;
  onTab: (tab: ClubTab) => void;
  onClose: () => void;
}

/** The profile of one club. Mount it with `key={uid}` so a new club starts at the top. */
export function ClubProfile({ club, version, tab, onTab, onClose }: ClubProfileProps) {
  const loaded = useProfileDetails<ClubDetails>("club_details", club.uid, version);
  const editing = useProfileEditing("club", club.uid);
  const base = useId();
  const details = loaded?.details;
  const info: ClubSummary = details ?? club;
  const vitals = [info.founded !== null && text.founded(info.founded), `${profileText.uid} ${info.uid}`];

  // The club is edited on the Overview and the Stadium tab, which have its inputs: editing starts on the Overview
  // unless the Stadium tab is open, and the Squad tab is not offered until it is done.
  const locked = editing.stage !== "view";
  useEffect(() => {
    if (locked && tab === "squad") {
      onTab("overview");
    }
  }, [locked, tab, onTab]);
  const shown = locked && tab === "squad" ? "overview" : tab;
  const panel: PanelIds = { id: `${base}-panel`, labelledBy: `${base}-${shown}` };

  return (
    <ProfileFrame
      picture={<ClubLogo uid={info.uid} name={info.name} size={PICTURE_SIZE.clubLogo} />}
      name={info.name}
      fullName={info.shortName}
      badge={info.women ? <span className="tag women">{text.womensClub}</span> : undefined}
      vitals={vitals}
      side={
        // Laid out like the abilities of a person: a label and a bar with the number on it. While the club is
        // edited the number is an input, where it is shown.
        <div className="profile-abilities">
          <abbr title={text.reputationTitle}>{text.reputationShort}</abbr>
          <EditableValue field="reputation">
            <ReputationMeter value={info.reputation} />
          </EditableValue>
        </div>
      }
      tabs={details && <ClubTabs tab={shown} base={base} locked={locked} onSelect={onTab} />}
      wide={details !== undefined && shown === "squad"}
      loaded={loaded}
      onClose={onClose}
      editing={editing}
    >
      {details &&
        (shown === "squad" ? (
          <SquadTab clubUid={club.uid} version={version} panel={panel} />
        ) : shown === "stadium" ? (
          <StadiumTab club={club.uid} stadium={details.stadium} panel={panel} />
        ) : (
          <ClubBody details={details} panel={panel} />
        ))}
    </ProfileFrame>
  );
}

interface ClubTabsProps {
  tab: ClubTab;
  /** The start of the ids of the tabs, which the panel they open names itself by (see `PanelIds`). */
  base: string;
  /** Whether the club is being edited. */
  locked: boolean;
  onSelect: (tab: ClubTab) => void;
}

/** The tabs under the header. The arrow keys move between them, as in any tab list. */
function ClubTabs({ tab, base, locked, onSelect }: ClubTabsProps) {
  const buttons = useRef<Partial<Record<ClubTab, HTMLButtonElement | null>>>({});
  // The Squad tab is out of reach while the club is edited.
  const offered = TABS.filter((id) => !(locked && id === "squad"));

  const keyDown = (event: KeyboardEvent) => {
    const at = offered.indexOf(tab);
    const target =
      event.key === "ArrowRight"
        ? offered[(at + 1) % offered.length]
        : event.key === "ArrowLeft"
          ? offered[(at - 1 + offered.length) % offered.length]
          : event.key === "Home"
            ? offered[0]
            : event.key === "End"
              ? offered[offered.length - 1]
              : undefined;
    if (target !== undefined) {
      event.preventDefault();
      onSelect(target);
      buttons.current[target]?.focus();
    }
  };

  return (
    <div className="profile-tabs" role="tablist" aria-label={text.tabsLabel} onKeyDown={keyDown}>
      {TABS.map((id) => {
        const unavailable = locked && id === "squad";
        return (
          <button
            key={id}
            ref={(button) => {
              buttons.current[id] = button;
            }}
            type="button"
            role="tab"
            id={`${base}-${id}`}
            className="profile-tab"
            aria-selected={id === tab}
            aria-controls={`${base}-panel`}
            // Only the tab that is open is a Tab stop; the arrow keys reach the other.
            tabIndex={id === tab ? 0 : -1}
            disabled={unavailable}
            title={unavailable ? text.squadLocked : undefined}
            onClick={() => onSelect(id)}
          >
            {text.tabs[id]}
          </button>
        );
      })}
    </div>
  );
}

/** Two columns: what the club has on the left, the people around it on the right. */
function ClubBody({ details, panel }: { details: ClubDetails; panel: PanelIds }) {
  return (
    <div className="profile-split" role="tabpanel" id={panel.id} aria-labelledby={panel.labelledBy}>
      <div className="profile-stack">
        <Facilities facilities={details.facilities} />
        <FinancesGroup club={details.uid} finances={details.finances} />
      </div>
      <div className="profile-stack">
        <Supporters traits={details.supporterTraits} mix={details.supporterMix} />
        <ClubInfo details={details} />
      </div>
    </div>
  );
}

/** The keys of `labels` that `values` has, in the order of `labels`. */
function knownKeys<K extends string>(labels: Record<K, string>, values: Record<string, number>): K[] {
  return (Object.keys(labels) as K[]).filter((key) => values[key] !== undefined);
}

/**
 * Rating rows for the keys of `labels` that `values` has; a rating the game does not give is left out. The rows of
 * values the editor can change have `fieldGroup` (e.g. "facilities"): their field is `fieldGroup.key`.
 */
function ratingRows(
  labels: Record<string, string>,
  values: Record<string, number>,
  fieldGroup?: string,
): RatingRow[] {
  return knownKeys(labels, values).map((key) => ({
    key,
    field: fieldGroup === undefined ? undefined : `${fieldGroup}.${key}`,
    label: labels[key],
    value: values[key],
  }));
}

/** The club's facilities as ratings; inputs while the club is edited. */
function Facilities({ facilities }: { facilities: Record<string, number> }) {
  const rows = ratingRows(CLUB_FACILITY_LABELS, facilities, "facilities");
  return rows.length > 0 ? <RatingGroup title={text.facilities} rows={rows} /> : null;
}

function Supporters({ traits, mix }: { traits: Record<string, number>; mix: Record<string, number> }) {
  const traitRows = ratingRows(SUPPORTER_TRAIT_LABELS, traits);
  const segments = mixSegments(mix);
  if (traitRows.length === 0 && segments.length === 0) {
    return null;
  }
  return (
    <section className="profile-group">
      <h3>{text.supporters}</h3>
      {traitRows.length > 0 && <RatingList rows={traitRows} />}
      {segments.length > 0 && <SupporterMix segments={segments} />}
    </section>
  );
}

interface MixSegment {
  key: SupporterMixKey;
  label: string;
  /** Percent of the supporters. */
  value: number;
}

/** The kinds of supporters the game gives, in the order of the bar; nothing when it gives none. */
function mixSegments(mix: Record<string, number>): MixSegment[] {
  const segments = knownKeys(SUPPORTER_MIX_LABELS, mix).map((key) => ({
    key,
    label: SUPPORTER_MIX_LABELS[key],
    value: mix[key],
  }));
  // A mix of zeroes tells no more than none at all.
  return segments.some((segment) => segment.value > 0) ? segments : [];
}

/**
 * The supporter mix as one stacked bar, split by percent, with a legend that has every share as
 * text. Each kind keeps its colour (see `.mix-*` in App.css), whatever the others are.
 */
function SupporterMix({ segments }: { segments: MixSegment[] }) {
  return (
    <div className="mix">
      <h4>{text.supporterMix}</h4>
      {/* The legend has every share as text, so the bar is only for the eye. */}
      <div className="mix-bar" aria-hidden="true">
        {segments
          .filter((segment) => segment.value > 0)
          .map(({ key, label, value }) => (
            <span
              key={key}
              className={`mix-${key}`}
              style={{ flexGrow: value }}
              title={text.mixTooltip(label, text.percent(value))}
            />
          ))}
      </div>
      <ul className="legend mix-legend">
        {segments.map(({ key, label, value }) => (
          <li key={key} className={`mix-${key}`}>
            <span className="swatch" />
            {label}
            <span className="range">{text.percent(value)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * The club's money; the four amounts the editor can change, and the share of transfer revenue the board makes
 * available where the layout places it, are inputs while the club is edited, and the money can be frozen.
 */
function FinancesGroup({ club, finances }: { club: number; finances: Finances | null }) {
  return (
    <section className="profile-group">
      <h3>{text.finances}</h3>
      {finances ? (
        <>
          <dl className="stats">
            <Fact label={text.balance}>
              <EditableValue field="finance.balance">
                <span className={finances.balance < 0 ? "negative" : undefined}>{formatMoney(finances.balance)}</span>
              </EditableValue>
            </Fact>
            <Fact label={text.transferBudget}>
              <EditableValue field="finance.transfer_budget">
                {/* FM stores -1 when the board has not set a transfer budget. */}
                {finances.transferBudget < 0 ? profileText.notSet : formatMoney(finances.transferBudget)}
              </EditableValue>
            </Fact>
            <Fact label={perWeek(text.wageBudget)}>
              <EditableValue field="finance.wage_budget">{formatMoney(finances.wageBudget)}</EditableValue>
            </Fact>
            <Fact label={perWeek(text.totalWageBudget)}>
              <EditableValue field="finance.total_wage_budget">{formatMoney(finances.totalWageBudget)}</EditableValue>
            </Fact>
            {finances.transferRevenue !== null && (
              <Fact label={text.transferRevenue}>
                <EditableValue field="finance.transfer_revenue">{text.percent(finances.transferRevenue)}</EditableValue>
              </Fact>
            )}
            <Fact label={text.ticketPrice}>{formatTicketPrice(finances.ticketPrice)}</Fact>
          </dl>
          <FreezeMoney club={club} />
          <p className="note">{profileText.wageNote(converts)}</p>
        </>
      ) : (
        <p className="note">{text.noFinances}</p>
      )}
    </section>
  );
}

/**
 * The Stadium tab: the stadium the senior team plays at, its name, which can be changed in place, and its capacities,
 * inputs while the club is edited.
 */
function StadiumTab({ club, stadium, panel }: { club: number; stadium: Stadium | null; panel: PanelIds }) {
  return (
    <div className="profile-stack stadium-tab" role="tabpanel" id={panel.id} aria-labelledby={panel.labelledBy}>
      <StadiumGroup club={club} stadium={stadium} />
    </div>
  );
}

/** The stadium the senior team plays at: its name, and its capacities, inputs while the club is edited. */
function StadiumGroup({ club, stadium }: { club: number; stadium: Stadium | null }) {
  return (
    <section className="profile-group">
      <h3>{text.stadium}</h3>
      {stadium && <StadiumNameRow club={club} name={stadium.name} />}
      {stadium ? (
        <dl className="stats">
          <Fact label={text.capacity}>
            <EditableValue field="stadium.capacity">{formatNumber(stadium.capacity)}</EditableValue>
          </Fact>
          <Fact label={text.seating}>
            <EditableValue field="stadium.seating">{formatNumber(stadium.seating)}</EditableValue>
          </Fact>
          <Fact label={text.used} title={text.usedTitle}>
            <EditableValue field="stadium.used">{stadium.used === 0 ? text.usedAll : formatNumber(stadium.used)}</EditableValue>
          </Fact>
          <Fact label={text.expansion}>
            <EditableValue field="stadium.expansion">{formatNumber(stadium.expansion)}</EditableValue>
          </Fact>
        </dl>
      ) : (
        <p className="note">{text.noStadium}</p>
      )}
    </section>
  );
}

/** How many bytes of UTF-8 `text` takes, as the game keeps a name. */
const utf8Length = (value: string) => new TextEncoder().encode(value).length;

/**
 * A stadium's name, with Rename while the game can be written to: under it, an input, Save and Cancel, and a note of
 * its length. Any name of 1 to 80 bytes (see `stadium_name`): written over the old one where it fits, given a string
 * of its own otherwise. Where the editor cannot rename it (a shared name, where it cannot take memory), a note says
 * so. Written at once, and logged. An (i) after it says how a new name lasts: the app gives it again each time the
 * game loads the save, while it is open.
 */
function StadiumNameRow({ club, name }: { club: number; name: string | null }) {
  const { editable, onEdited } = useContext(EditorContext);
  const [room, setRoom] = useState<StadiumName | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const noteId = useId();

  // What the name may become, read again whenever it changes.
  useEffect(() => {
    let alive = true;
    setRoom(null);
    stadiumName(club).then(
      (found) => alive && setRoom(found),
      () => alive && setRoom(null),
    );
    return () => {
      alive = false;
    };
  }, [club, name]);

  // The input takes the focus when it comes.
  const renamingNow = draft !== null;
  useEffect(() => {
    if (renamingNow) {
      input.current?.focus();
    }
  }, [renamingNow]);

  // Where the editor cannot rename it (a name other things share, where it cannot take memory), the note says why.
  const shared = room !== null && !room.renamable;
  const row = (
    <dl className="stats stadium-name-row">
      <Fact label={text.stadiumName}>
        <span className="stadium-name">
          <span>{name ?? profileText.none}</span>
          {room !== null && !shared && draft === null && (
            <button
              type="button"
              className="heal-toggle"
              disabled={!editable}
              title={text.renameTitle}
              onClick={() => {
                setError(null);
                setDraft(room.name);
              }}
            >
              {text.rename}
            </button>
          )}
          {room !== null && !shared && <InfoTip label={text.renameInfoLabel} note={text.renameInfo} />}
        </span>
      </Fact>
    </dl>
  );
  if (draft === null) {
    return (
      <>
        {row}
        {shared && <p className="note">{text.nameShared(room.holders)}</p>}
      </>
    );
  }

  const bytes = utf8Length(draft);
  const fits = room !== null && bytes >= room.minBytes && bytes <= room.maxBytes;
  const clean = draft.trim() === draft && draft.length > 0;
  const changed = draft !== room?.name;
  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await renameStadium(club, draft);
      setDraft(null);
      onEdited();
    } catch (e) {
      setError(describeError(e));
    } finally {
      setSaving(false);
    }
  };
  return (
    <>
      {row}
      <div className="stadium-rename">
        <div className="stadium-rename-line">
          <input
            ref={input}
            type="text"
            className="stadium-name-input"
            aria-label={text.stadiumName}
            aria-describedby={noteId}
            aria-invalid={!fits || !clean || undefined}
            value={draft}
            disabled={saving}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && fits && clean && changed && !saving) {
                void save();
              } else if (e.key === "Escape") {
                e.stopPropagation();
                setDraft(null);
              }
            }}
          />
          <button
            type="button"
            className="primary"
            disabled={saving || !fits || !clean || !changed}
            onClick={() => void save()}
          >
            {saving ? text.renaming : text.saveName}
          </button>
          <button type="button" disabled={saving} onClick={() => setDraft(null)}>
            {text.cancelRename}
          </button>
        </div>
        {room !== null && (
          <p id={noteId} className={fits && clean ? "move-note" : "move-note invalid"}>
            {text.nameLength(bytes, room.minBytes, room.maxBytes)}
          </p>
        )}
        {error !== null && (
          <p className="move-note invalid" role="alert">
            {error}
          </p>
        )}
      </div>
    </>
  );
}

/**
 * Freezing the club's money: while the app is open and connected, its balance does not go down. Offered while the game
 * can be written to; once frozen, a note says what it does.
 */
function FreezeMoney({ club }: { club: number }) {
  const { editable } = useContext(EditorContext);
  // Whether the club's money is frozen; null until the app has said.
  const [frozen, setFrozen] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    frozenClubs()
      .then((clubs) => alive && setFrozen(clubs.includes(club)))
      .catch(() => alive && setFrozen(false));
    return () => {
      alive = false;
    };
  }, [club]);

  if (!editable || frozen === null) {
    return null;
  }
  const toggle = async (on: boolean) => {
    setError(null);
    try {
      setFrozen((await freezeMoney(club, on)).includes(club));
    } catch (e) {
      setError(describeError(e));
    }
  };
  return (
    <>
      <div className="freeze-line">
        <label className="freeze-toggle" title={text.freezeNote}>
          <input type="checkbox" checked={frozen} onChange={(e) => void toggle(e.currentTarget.checked)} />
          <span>{text.freeze}</span>
        </label>
        <InfoTip label={appText.freezeInfo.label} note={appText.freezeInfo.note} />
      </div>
      {frozen && <p className="note">{text.freezeNote}</p>}
      {error && (
        <p className="note error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}

/** The label of an amount per week: the name, with "per week" in small print under it. */
function perWeek(label: string): ReactNode {
  return (
    <>
      {label}
      <span className="unit">{text.perWeek}</span>
    </>
  );
}

/** A ticket price: a whole amount as it is, anything else with two decimals ("£41.80"). */
function formatTicketPrice(price: number): string {
  return Number.isInteger(inCurrency(price))
    ? formatMoney(price)
    : formatMoney(price, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function ClubInfo({ details }: { details: ClubDetails }) {
  const count = (value: number | null) => (value === null ? profileText.none : formatNumber(value));
  return (
    <section className="profile-group">
      <h3>{text.info}</h3>
      <dl className="stats">
        <Fact label={text.seasonTickets}>{count(details.seasonTickets)}</Fact>
        <Fact label={text.socialMedia}>{count(details.socialMedia)}</Fact>
      </dl>
    </section>
  );
}
