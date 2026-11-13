// The Squad tab of a club's profile: the players of each of its teams with the values a squad screen shows, the actions
// on a whole team (heal everyone, lift every ban, give everyone the best morale, rest everyone), the boxes that freeze
// the whole team, and the captains. What it writes is logged like any change, and undone from the Changes view, or at
// once from the line under the buttons. A player's name opens his profile over the squad, which stays as it was behind
// it.

import { useContext, useEffect, useMemo, useRef, useState, type Ref } from "react";
import { Ability } from "./Ability";
import { CaptainMenu } from "./CaptainMenu";
import {
  changeLog,
  clubSquads,
  freezeTeam,
  refreshPlayer,
  setCaptain,
  teamAction,
  undoBatch,
  undoChange,
} from "./editing";
import { describeError, formatNumber } from "./format";
import { intlLocale } from "./i18n";
import { InfoTip } from "./InfoTip";
import {
  appText,
  fieldLabel,
  massEditText,
  positionLabel,
  profileText,
  squadText as text,
  swapNote,
  teamLabel,
} from "./labels";
import { undoFailureLines } from "./massDraft";
import { percentText } from "./MedicalGroup";
import { PlayerDialog } from "./PlayerProfile";
import { ResizableTop } from "./ResizableTop";
import type { Loaded, PanelIds } from "./ProfileParts";
import { SortHeader } from "./SortHeader";
import type {
  BatchUndo,
  CaptainRole,
  ClubTeam,
  FreezeKind,
  LoggedChange,
  MassReport,
  SquadPlayer,
  SquadSort,
  TeamAction,
} from "./types";
import { EditorContext } from "./useProfileEditing";

/**
 * The club's teams with their players, read again whenever `version` changes: the game data was read or written. Like
 * `useProfileDetails`, but through the squad command, with a failure worded in the language of the window.
 */
function useSquads(club: number, version: number): Loaded<ClubTeam[]> | null {
  const [loaded, setLoaded] = useState<Loaded<ClubTeam[]> | null>(null);

  useEffect(() => {
    let cancelled = false;
    clubSquads(club).then(
      (details) => {
        if (!cancelled) {
          setLoaded({ details });
        }
      },
      (e) => {
        if (!cancelled) {
          setLoaded({ error: describeError(e) });
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [club, version]);

  return loaded;
}

/** How the table is sorted. */
interface SquadOrder {
  by: SquadSort;
  ascending: boolean;
}

/** What the table is in until a column header is clicked: the best players first. */
const DEFAULT_ORDER: SquadOrder = { by: "currentAbility", ascending: false };

/** The columns whose first click sorts from A to Z or from the goalkeepers; the numbers start with the highest. */
const FIRST_ASCENDING: ReadonlySet<SquadSort> = new Set<SquadSort>(["name", "position"]);

/** Goalkeepers first, then the defence, the midfield and the attack: the order a squad list reads in. */
const POSITION_ORDER: readonly string[] = [
  "gk", "sw", "dl", "dc", "dr", "wbl", "wbr", "dm", "ml", "mc", "mr", "aml", "amc", "amr", "st",
];

const collator = new Intl.Collator(intlLocale);

/** What the player is sorted by in the column `by`; null for what the game does not give: no age, no position. */
function sortValue(player: SquadPlayer, by: SquadSort): string | number | null {
  switch (by) {
    case "name":
      return player.name;
    case "position": {
      const at = POSITION_ORDER.indexOf(player.positions[0]);
      return at < 0 ? null : at;
    }
    case "age":
      return player.age;
    case "currentAbility":
      return player.currentAbility;
    case "potentialAbility":
      return player.potentialAbility;
    case "condition":
      return player.condition;
    case "matchSharpness":
      return player.matchSharpness;
    case "jadedness":
      return player.jadedness;
    case "morale":
      return player.morale;
  }
}

/** The players in the order `order` asks for; equals keep the best players first, then the names in order. */
function sorted(players: readonly SquadPlayer[], { by, ascending }: SquadOrder): SquadPlayer[] {
  return [...players].sort((a, b) => {
    const x = sortValue(a, by);
    const y = sortValue(b, by);
    if (x === null || y === null) {
      // What is not known goes last, whichever way the table is sorted.
      if (x !== y) {
        return x === null ? 1 : -1;
      }
    } else if (x !== y) {
      const order = typeof x === "string" ? collator.compare(x, String(y)) : x - Number(y);
      return ascending ? order : -order;
    }
    return b.currentAbility - a.currentAbility || collator.compare(a.name, b.name);
  });
}

/** The role the player holds in the team, if any. */
function roleOf(team: ClubTeam, uid: number): CaptainRole | null {
  return team.captain === uid ? "captain" : team.viceCaptain === uid ? "viceCaptain" : null;
}

/** The field of the club that names each role (see `fieldLabel`). */
const ROLE_FIELD: Record<CaptainRole, string> = { captain: "captain", viceCaptain: "vice_captain" };

/** Takes back what a result says was written. */
type Undo = () => Promise<BatchUndo>;

/**
 * What an action, a captaincy or an undo came to, for the line under the buttons. It is shown under the team it was
 * made to only.
 */
interface Result {
  /** The team's place in the club's list (see `ClubTeam.index`). */
  team: number;
  message: string;
  /** Whether the message is why it did not happen. */
  failed: boolean;
  /** Under the message: what the game refused, as lines of text. */
  problems: string[];
  /** Null for what there is nothing to take back, or what was taken back already. */
  undo: Undo | null;
}

/** Most refusals the line under the buttons lists. */
const PROBLEMS_SHOWN = 5;

/** The changes of a team action the game refused: whose and why; the first few, then how many more. */
function refusalLines(failed: MassReport["failed"]): string[] {
  const lines = failed.slice(0, PROBLEMS_SHOWN).map(({ name, error }) => text.refused(name, describeError(error)));
  return failed.length > PROBLEMS_SHOWN ? [...lines, massEditText.more(failed.length - PROBLEMS_SHOWN)] : lines;
}

/** What a captaincy came to: who took the role and, when the two captains swapped, what became of the other. */
function captainMessage(role: CaptainRole, player: string, captaincy: LoggedChange["captaincy"]): string {
  const taken = (role === "captain" ? text.nowCaptain : text.nowViceCaptain)(captaincy?.to ?? player);
  return captaincy?.swapped ? `${taken} · ${swapNote(role, captaincy.from)}` : taken;
}

interface SquadTabProps {
  clubUid: number;
  /** Changes when the game data is read again, so the squad is too. */
  version: number;
  panel: PanelIds;
  /** The team FM's screen shows, by its place in the club's list, to switch to (see followView.ts). */
  followedTeam?: { team: number | null; seq: number } | null;
}

/** The squad of the club `clubUid`: a switch between its teams, the team's summary and actions, and its table. */
export function SquadTab({ clubUid, version, panel, followedTeam }: SquadTabProps) {
  const { editable, onEdited } = useContext(EditorContext);
  const loaded = useSquads(clubUid, version);
  // The team that is shown, by its place in the club's list.
  const [teamIndex, setTeamIndex] = useState(0);
  const [order, setOrder] = useState(DEFAULT_ORDER);
  // Whether a write is under way, or done and waiting for the squad to be read again.
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const alive = useRef(true);
  const resultBox = useRef<HTMLDivElement>(null);
  // Set while a write may take the button that has focus out of play.
  const refocus = useRef(false);
  // The player whose profile is up over the squad, and the name that opened it.
  const [shown, setShown] = useState<SquadPlayer | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const wasShown = useRef(false);

  // A team on FM's screen is switched to, with no player's profile up over it.
  const followedSeq = followedTeam?.seq;
  const followedIndex = followedTeam?.team ?? null;
  useEffect(() => {
    if (followedSeq !== undefined && followedIndex !== null) {
      setTeamIndex(followedIndex);
      setShown(null);
    }
  }, [followedSeq, followedIndex]);

  const teams = loaded?.details;
  // A team that is no longer there (the game changed) gives way to the first one.
  const team = teams?.find((candidate) => candidate.index === teamIndex) ?? teams?.[0];
  const players = useMemo(() => (team ? sorted(team.players, order) : []), [team, order]);
  const shownResult = result !== null && result.team === team?.index ? result : null;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // A write stays busy until the squad has been read again, which gives new rows: pressed in between, the buttons would
  // work from numbers that are out of date.
  useEffect(() => {
    setBusy(false);
  }, [loaded]);

  // Once his profile is closed, focus goes back to the name that opened it, if the squad still has him. Doing it
  // earlier, with the dialog still up, would only make the dialog take focus back.
  useEffect(() => {
    if (wasShown.current && shown === null && opener.current?.isConnected) {
      opener.current.focus();
    }
    wasShown.current = shown !== null;
  }, [shown]);

  // A write can take the button that had focus out of play: Heal all once nobody is injured, Undo once it is done.
  // Focus then goes to the line under the buttons, so that someone using the keyboard stays in the tab.
  useEffect(() => {
    if (busy || !refocus.current) {
      return;
    }
    refocus.current = false;
    const focused = document.activeElement;
    if (!(focused instanceof HTMLElement) || focused === document.body || focused.matches(":disabled")) {
      resultBox.current?.focus();
    }
  });

  /** Writes for the team `forTeam`: busy until the squad is read again, with the result shown under its buttons. */
  const write = async (forTeam: number, task: () => Promise<Result>) => {
    if (busy) {
      return;
    }
    setBusy(true);
    refocus.current = true;
    try {
      const done = await task();
      if (alive.current) {
        setResult(done);
      }
      // The game has changed: the lists, the profile and the change log read it again, and so does the squad.
      onEdited();
    } catch (e) {
      if (alive.current) {
        setResult({ team: forTeam, message: describeError(e), failed: true, problems: [], undo: null });
        setBusy(false);
      }
    }
  };

  const act = (target: ClubTeam, action: TeamAction) =>
    write(target.index, async () => {
      const report = await teamAction(clubUid, target.index, action);
      return {
        team: target.index,
        message: text.written(report.applied),
        failed: false,
        problems: refusalLines(report.failed),
        // An action that wrote nothing has nothing to take back.
        undo: report.applied > 0 ? () => undoBatch(report.batch) : null,
      };
    });

  // One player rested as Refresh all rests the team, with the result and its undo under the buttons.
  const refresh = (target: ClubTeam, player: SquadPlayer) =>
    write(target.index, async () => {
      const report = await refreshPlayer(player.uid);
      return {
        team: target.index,
        message: `${player.name}: ${text.written(report.applied)}`,
        failed: false,
        problems: refusalLines(report.failed),
        undo: report.applied > 0 ? () => undoBatch(report.batch) : null,
      };
    });

  // Freezing writes nothing to the game itself, but the squad is read again all the same, for its rows to say so.
  const freeze = (target: ClubTeam, kind: FreezeKind, on: boolean) =>
    write(target.index, async () => {
      const done = await freezeTeam(clubUid, target.index, kind, on);
      const said =
        kind === "attributes"
          ? on
            ? text.frozeAttributes
            : text.unfrozeAttributes
          : kind === "fitness"
            ? on
              ? text.frozeFitness
              : text.unfrozeFitness
            : on
              ? text.frozePositions
              : text.unfrozePositions;
      return {
        team: target.index,
        message: said(done.changed),
        failed: false,
        problems: refusalLines(done.failed),
        undo: null,
      };
    });

  const undo = (forTeam: number, take: Undo) =>
    write(forTeam, async () => {
      const done = await take();
      // The log names the players behind the changes that could not be taken back.
      const log = done.failed.length > 0 ? await changeLog().catch(() => []) : [];
      return {
        team: forTeam,
        message: massEditText.undone(done.undone),
        failed: false,
        problems: undoFailureLines(done.failed, log),
        undo: null,
      };
    });

  const makeCaptain = (target: ClubTeam, role: CaptainRole, player: SquadPlayer) =>
    write(target.index, async () => {
      const change = await setCaptain(clubUid, target.index, role, player.uid);
      return {
        team: target.index,
        message: captainMessage(role, player.name, change.captaincy),
        failed: false,
        problems: [],
        undo: async () => {
          await undoChange(change.id);
          return { undone: 1, failed: [] };
        },
      };
    });

  const sortBy = (by: SquadSort) =>
    setOrder({ by, ascending: order.by === by ? !order.ascending : FIRST_ASCENDING.has(by) });
  const sortHeader = (by: SquadSort, className?: string) => (
    <SortHeader
      by={by}
      order={order}
      onSort={sortBy}
      label={text.columns[by]}
      title={text.sortBy(text.sortNames[by])}
      className={className}
    />
  );

  return (
    <div
      className="squad-tab"
      role="tabpanel"
      id={panel.id}
      aria-labelledby={panel.labelledBy}
      aria-busy={loaded === null || undefined}
    >
      {loaded === null && (
        <p className="profile-status" role="status">
          {text.loading}
        </p>
      )}
      {loaded?.error !== undefined && (
        <div className="banner" role="alert">
          {loaded.error}
        </div>
      )}
      {teams?.length === 0 && <p className="empty">{text.noTeams}</p>}

      {teams && team && (
        <>
          <ResizableTop
            storageKey="fmlibre.squadControls"
            className="squad-controls"
            label={text.resizeLabel}
            title={text.resizeTitle}
          >
            <TeamSummary teams={teams} team={team} onTeam={setTeamIndex} />

            <div className="squad-actions">
              <TeamActions team={team} editable={editable} busy={busy} onAction={(action) => void act(team, action)} />
              <p className="note squad-hint">{text.hint}</p>
            </div>

            <div className="squad-actions">
              <TeamFreezes
                team={team}
                editable={editable}
                busy={busy}
                onFreeze={(kind, on) => void freeze(team, kind, on)}
              />
              <p className="note squad-hint">{text.freezeHint}</p>
            </div>

            <ResultLine
              ref={resultBox}
              result={shownResult}
              busy={busy}
              onUndo={(take) => void undo(team.index, take)}
            />
          </ResizableTop>

          <div className="squad-table-wrap">
            <table className="grid squad-grid" aria-label={text.tableLabel(teamLabel(team))}>
              <thead>
                <tr>
                  {sortHeader("name", "col-name")}
                  {sortHeader("position")}
                  {sortHeader("age", "num")}
                  {sortHeader("currentAbility", "num")}
                  {sortHeader("potentialAbility", "num")}
                  {sortHeader("condition", "num")}
                  {sortHeader("matchSharpness", "num")}
                  {sortHeader("jadedness", "num")}
                  {sortHeader("morale", "num")}
                  <th className="col-actions">
                    <span className="sr-only">{text.columns.actions}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {players.map((player) => (
                  <SquadRow
                    key={player.uid}
                    player={player}
                    role={roleOf(team, player.uid)}
                    editable={editable}
                    busy={busy}
                    onCaptain={(role) => void makeCaptain(team, role, player)}
                    onRefresh={() => void refresh(team, player)}
                    onOpen={(from) => {
                      opener.current = from;
                      setShown(player);
                    }}
                  />
                ))}
              </tbody>
            </table>
            {players.length === 0 && <p className="empty">{text.empty}</p>}
          </div>
        </>
      )}

      {shown && <PlayerDialog player={shown} version={version} onClose={() => setShown(null)} />}
    </div>
  );
}

interface TeamSummaryProps {
  teams: readonly ClubTeam[];
  team: ClubTeam;
  onTeam: (index: number) => void;
}

/** The switch between the teams, and under it how many players the shown team lists and who its captains are. */
function TeamSummary({ teams, team, onTeam }: TeamSummaryProps) {
  const ages = team.players.flatMap((player) => (player.age === null ? [] : [player.age]));
  const average = ages.length > 0 ? ages.reduce((sum, age) => sum + age, 0) / ages.length : null;
  const summary = [
    text.players(team.players.length),
    average !== null && text.averageAge(formatNumber(average, { minimumFractionDigits: 1, maximumFractionDigits: 1 })),
  ]
    .filter(Boolean)
    .join(" · ");
  // A captain the team's list does not have (the game may name one who has left) is told by his UID.
  const nameOf = (uid: number | null) =>
    uid === null
      ? profileText.none
      : (team.players.find((player) => player.uid === uid)?.name ?? `${profileText.uid} ${uid}`);

  return (
    <div className="squad-summary">
      <div className="squad-teams" role="group" aria-label={text.teamsLabel}>
        {teams.map((candidate) => (
          <button
            key={candidate.index}
            type="button"
            aria-pressed={candidate.index === team.index}
            onClick={() => onTeam(candidate.index)}
          >
            {teamLabel(candidate)}
          </button>
        ))}
      </div>
      <p className="squad-count">{summary}</p>
      <p className="squad-captains">{text.captains(nameOf(team.captain), nameOf(team.viceCaptain))}</p>
    </div>
  );
}

interface TeamActionsProps {
  team: ClubTeam;
  /** Whether the game can be written to. */
  editable: boolean;
  /** Whether a write is under way. */
  busy: boolean;
  onAction: (action: TeamAction) => void;
}

/**
 * Heal all, Lift bans, Inspire all, Refresh all and Max attributes for the shown team. Heal all and Lift bans say how
 * many players are injured or banned and wait until somebody is; every one waits while the game cannot be written to,
 * and says why.
 */
function TeamActions({ team, editable, busy, onAction }: TeamActionsProps) {
  const { readOnlyNote } = useContext(EditorContext);
  const injured = team.players.filter((player) => player.injuries > 0).length;
  const banned = team.players.filter((player) => player.bans > 0).length;
  const empty = team.players.length === 0;
  // What a button's hover text says: why it waits, or else what it does.
  const hover = (does: string, waiting: string | null) => (!editable ? readOnlyNote : (waiting ?? does));

  return (
    <div className="squad-buttons" role="group" aria-label={text.actionsLabel}>
      <ActionButton
        label={text.healAll(injured)}
        title={hover(text.healAllTitle, injured === 0 ? text.healNone : null)}
        waiting={!editable || injured === 0}
        busy={busy}
        onPress={() => onAction("heal")}
      />
      <ActionButton
        label={text.liftAll(banned)}
        title={hover(text.liftAllTitle, banned === 0 ? text.liftNone : null)}
        waiting={!editable || banned === 0}
        busy={busy}
        onPress={() => onAction("liftBans")}
      />
      <ActionButton
        label={text.inspireAll}
        title={hover(text.inspireAllTitle, empty ? text.empty : null)}
        waiting={!editable || empty}
        busy={busy}
        onPress={() => onAction("inspire")}
      />
      <ActionButton
        label={text.refreshAll}
        title={hover(text.refreshAllTitle, empty ? text.empty : null)}
        waiting={!editable || empty}
        busy={busy}
        onPress={() => onAction("refresh")}
      />
      <ActionButton
        label={text.maxAll}
        title={hover(text.maxAllTitle, empty ? text.empty : null)}
        waiting={!editable || empty}
        busy={busy}
        onPress={() => onAction("maxAttributes")}
      />
    </div>
  );
}

interface TeamFreezesProps {
  team: ClubTeam;
  /** Whether the game can be written to. */
  editable: boolean;
  /** Whether a write is under way. */
  busy: boolean;
  onFreeze: (kind: FreezeKind, on: boolean) => void;
}

/**
 * The boxes that freeze the attributes, the energy and match sharpness, and the positions of every player in the shown
 * team. A box is ticked when all of them are frozen that way, and half ticked, with how many are, when some are;
 * ticking it then freezes the others too. They wait while the game cannot be written to, and say why.
 */
function TeamFreezes({ team, editable, busy, onFreeze }: TeamFreezesProps) {
  const { readOnlyNote } = useContext(EditorContext);
  const total = team.players.length;
  const boxes: { kind: FreezeKind; label: string; title: string; frozen: number }[] = [
    {
      kind: "attributes",
      label: text.freezeAttributes,
      title: text.freezeAttributesTitle,
      frozen: team.players.filter((player) => player.attributesFrozen).length,
    },
    {
      kind: "fitness",
      label: text.freezeFitness,
      title: text.freezeFitnessTitle,
      frozen: team.players.filter((player) => player.fitnessFrozen).length,
    },
    {
      kind: "positions",
      label: text.freezePositions,
      title: text.freezePositionsTitle,
      frozen: team.players.filter((player) => player.positionsFrozen).length,
    },
  ];

  return (
    <div className="squad-freezes" role="group" aria-label={text.freezeLabel}>
      {boxes.map(({ kind, label, title, frozen }) => (
        <FreezeBox
          key={kind}
          label={label}
          title={!editable ? readOnlyNote : total === 0 ? text.empty : title}
          frozen={frozen}
          total={total}
          waiting={!editable || total === 0}
          busy={busy}
          onChange={(on) => onFreeze(kind, on)}
        />
      ))}
      <InfoTip label={appText.freezeInfo.label} note={appText.freezeInfo.note} />
    </div>
  );
}

interface FreezeBoxProps {
  label: string;
  /** Hover text. */
  title: string;
  /** How many of the team's `total` players are frozen this way. */
  frozen: number;
  total: number;
  /** Whether it can do nothing now: the box is disabled. */
  waiting: boolean;
  /** Whether a write is under way: the box is only dimmed (aria-disabled), so that it keeps focus. */
  busy: boolean;
  onChange: (on: boolean) => void;
}

/** One of the boxes that freeze the whole team: ticked, half ticked with a count, or empty. */
function FreezeBox({ label, title, frozen, total, waiting, busy, onChange }: FreezeBoxProps) {
  const box = useRef<HTMLInputElement>(null);
  const some = frozen > 0 && frozen < total;

  // A box is half ticked only through the DOM.
  useEffect(() => {
    if (box.current) {
      box.current.indeterminate = some;
    }
  }, [some]);

  return (
    <label className="freeze-toggle" title={title}>
      <input
        ref={box}
        type="checkbox"
        checked={total > 0 && frozen === total}
        disabled={waiting}
        aria-disabled={busy || undefined}
        onChange={(e) => !busy && onChange(e.currentTarget.checked)}
      />
      <span>{label}</span>
      {some && <span className="freeze-count">{text.frozenCount(frozen, total)}</span>}
    </label>
  );
}

interface ActionButtonProps {
  label: string;
  /** Hover text. */
  title: string;
  /** Whether there is nothing it can do now: the button is disabled. */
  waiting: boolean;
  /** Whether a write is under way. */
  busy: boolean;
  onPress: () => void;
}

/**
 * One of the actions on the team. While a write is under way it is only dimmed (aria-disabled): a disabled button loses
 * focus.
 */
function ActionButton({ label, title, waiting, busy, onPress }: ActionButtonProps) {
  return (
    <button
      type="button"
      className="squad-action"
      disabled={waiting}
      aria-disabled={busy || undefined}
      aria-busy={busy || undefined}
      title={title}
      onClick={() => !busy && onPress()}
    >
      {label}
    </button>
  );
}

interface ResultLineProps {
  /** What the last write came to, for the team that is shown; none when there is nothing to say. */
  result: Result | null;
  busy: boolean;
  onUndo: (take: Undo) => void;
  /** The line takes focus when the button that had it is gone (see `SquadTab`). */
  ref: Ref<HTMLDivElement>;
}

/**
 * What the last write came to, with the button that takes it back and what the game refused. A live region that is
 * always there, so that it is read out when it fills; it takes no room while it is empty.
 */
function ResultLine({ result, busy, onUndo, ref }: ResultLineProps) {
  return (
    <div ref={ref} className="squad-result" role="status" tabIndex={-1}>
      {result && (
        <>
          <div className="squad-result-line">
            <p className={result.failed ? "error" : undefined}>{result.message}</p>
            {result.undo && (
              <button
                type="button"
                className="squad-action"
                aria-disabled={busy || undefined}
                title={text.undoTitle}
                onClick={() => result.undo && onUndo(result.undo)}
              >
                {text.undo}
              </button>
            )}
          </div>
          {result.problems.length > 0 && (
            <ul className="edit-problems">
              {result.problems.map((problem, index) => (
                <li key={index}>{problem}</li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}

interface SquadRowProps {
  player: SquadPlayer;
  /** The role he holds in the shown team. */
  role: CaptainRole | null;
  editable: boolean;
  busy: boolean;
  onCaptain: (role: CaptainRole) => void;
  /** Rests him as Refresh all rests the team. */
  onRefresh: () => void;
  /** Opens his profile over the squad; `from` is the name, which takes focus back once it is closed. */
  onOpen: (from: HTMLButtonElement) => void;
}

/**
 * One player: his name, which opens his profile over the squad, with his tags and badge by it, his values, the button
 * that rests him, and the menu that makes him captain.
 */
function SquadRow({ player, role, editable, busy, onCaptain, onRefresh, onOpen }: SquadRowProps) {
  const positions = player.positions.map(positionLabel);
  const fullName = player.fullName !== null && player.fullName !== player.name ? player.fullName : null;
  return (
    // A player who does not play for the team is told apart by his tag, and dimmed.
    <tr className={player.playsHere ? undefined : "away"}>
      <td className="col-name" title={fullName === null ? player.name : `${player.name}\n${fullName}`}>
        <div className="squad-name">
          <button
            type="button"
            className="name name-link"
            aria-haspopup="dialog"
            title={text.openProfile}
            onClick={(event) => onOpen(event.currentTarget)}
          >
            {player.name}
          </button>
          {role !== null && (
            <abbr className={`role-badge ${role}`} title={fieldLabel("club", ROLE_FIELD[role])}>
              {role === "captain" ? text.captainShort : text.viceCaptainShort}
            </abbr>
          )}
          <Tags player={player} />
        </div>
      </td>
      {/* The main position; the hover text has the others he can play. */}
      <td title={positions.join(" ") || undefined}>
        {positions[0] ?? <span className="muted">{profileText.none}</span>}
      </td>
      <td className="num">{player.age ?? profileText.none}</td>
      <td className="num">
        <Ability value={player.currentAbility} />
      </td>
      <td className="num">
        <Ability value={player.potentialAbility} />
      </td>
      <td className="num">{percentText(player.condition)}</td>
      <td className="num">{percentText(player.matchSharpness)}</td>
      <td className="num">{formatNumber(player.jadedness)}</td>
      <td className="num">{profileText.morale(player.morale)}</td>
      <td className="col-actions">
        <div className="squad-row-actions">
          <button
            type="button"
            className="icon-button squad-menu-toggle"
            aria-label={text.refreshPlayer(player.name)}
            // While a change is under way the button is only dimmed (aria-disabled): a disabled button loses focus.
            aria-disabled={busy || undefined}
            disabled={!editable}
            title={text.refreshPlayerTitle}
            onClick={() => {
              if (!busy) {
                onRefresh();
              }
            }}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M20 12a8 8 0 1 1-2.34-5.66" />
              <path d="M20 4v5h-5" />
            </svg>
          </button>
          {player.playsHere && (
            <CaptainMenu name={player.name} holds={role} editable={editable} busy={busy} onPick={onCaptain} />
          )}
        </div>
      </td>
    </tr>
  );
}

/**
 * What is out of the ordinary about a player: his injuries, bans, a loan, playing for another team of the club, or
 * being frozen.
 */
function Tags({ player }: { player: SquadPlayer }) {
  return (
    <>
      {player.injuries > 0 && (
        <span className="tag injured" title={text.injuredTitle(player.injuries)}>
          {text.injured}
        </span>
      )}
      {player.bans > 0 && (
        <span className="tag injured" title={text.bannedTitle(player.bans)}>
          {text.banned}
        </span>
      )}
      {player.loan === "out" ? (
        <span className="tag loan" title={text.onLoanTitle}>
          {text.onLoan}
        </span>
      ) : player.loan === "in" ? (
        <span className="tag loan" title={text.loaneeTitle}>
          {text.loanee}
        </span>
      ) : (
        !player.playsHere && (
          <span className="tag" title={text.elsewhereTitle}>
            {text.elsewhere}
          </span>
        )
      )}
      {(player.attributesFrozen || player.fitnessFrozen || player.positionsFrozen) && (
        <span
          className="tag frozen"
          title={text.frozenTitle(player.attributesFrozen, player.fitnessFrozen, player.positionsFrozen)}
        >
          {text.frozen}
        </span>
      )}
    </>
  );
}
