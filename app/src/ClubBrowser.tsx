import { useEffect, useState } from "react";
import { ClubProfile, readTab, type ClubTab } from "./ClubProfile";
import { ExportButton } from "./ExportDialog";
import { clubListText, profileText } from "./labels";
import { ListBrowser, type Followed } from "./ListBrowser";
import { MassEditButton } from "./MassEditDialog";
import { ClubLogo, PICTURE_SIZE, WithPicture } from "./Picture";
import { ReputationMeter } from "./ProfileParts";
import { restored, useSnapshot } from "./restore";
import type { ClubFilter, ClubSummary } from "./types";

const columns = clubListText.columns;
const filters = clubListText.filters;

/** Which clubs the list shows. */
type ClubKind = "all" | "men" | "women";

/** The kind of clubs saved before the language was switched; all of them when there is none. */
const readKind = (value: unknown): ClubKind => (value === "men" || value === "women" ? value : "all");

/** The club FM's screen shows, and the place in the club's list of the team it shows, if any (see followView.ts). */
export type FollowedClub = Followed<ClubSummary> & { team: number | null };

interface ClubBrowserProps {
  version: number;
  active: boolean;
  /** The club FM's screen shows, to open: on its squad, at that team, when the screen shows a team. */
  followed?: FollowedClub | null;
}

/** The clubs table with the profile of the selected club beside it. */
export function ClubBrowser({ version, active, followed }: ClubBrowserProps) {
  // It starts as it was before the language was switched, if it was (see `restore.ts`).
  const [kind, setKind] = useState<ClubKind>(() => restored("clubKind", readKind));
  useSnapshot("clubKind", () => kind);
  // The tab of the club profile is kept here, so that the next club picked opens on the squad when this one did.
  const [tab, setTab] = useState<ClubTab>(() => restored("clubTab", readTab));
  useSnapshot("clubTab", () => tab);
  // A team on FM's screen opens the club's squad.
  const followedSeq = followed?.seq;
  const followedTeam = followed?.team ?? null;
  useEffect(() => {
    if (followedSeq !== undefined && followedTeam !== null) {
      setTab("squad");
    }
  }, [followedSeq, followedTeam]);
  const filter: ClubFilter | undefined = kind === "all" ? undefined : { women: kind === "women" };

  return (
    <ListBrowser<ClubSummary>
      name="clubs"
      command="search_clubs"
      requestArgs={{ filter }}
      version={version}
      active={active}
      placeholder={clubListText.search}
      summary={clubListText.summary}
      empty={(query) => clubListText.empty(query, filter !== undefined)}
      filterBar={
        <div className="filter-bar">
          <div className="filter-fields">
            <select aria-label={filters.kind} value={kind} onChange={(e) => setKind(e.currentTarget.value as ClubKind)}>
              <option value="all">{filters.all}</option>
              <option value="men">{filters.men}</option>
              <option value="women">{filters.women}</option>
            </select>
          </div>
        </div>
      }
      toolbarActions={(list) => (
        <>
          <ExportButton entity="club" list={list} args={{ filter }} />
          <MassEditButton entity="club" filters={{ clubFilter: filter }} list={list} />
        </>
      )}
      tableClass="club-grid"
      header={
        <>
          <th className="col-name">{columns.name}</th>
          <th className="num col-rep">{columns.reputation}</th>
          <th className="num col-founded">{columns.founded}</th>
          <th className="num col-uid">{columns.uid}</th>
        </>
      }
      cells={(c) => <ClubCells club={c} />}
      followed={followed}
      profile={(c, panel) => (
        <ClubProfile
          club={c}
          version={panel.version}
          tab={tab}
          onTab={setTab}
          onClose={panel.onClose}
          followedTeam={followed && followed.row.uid === c.uid && followed.team !== null ? followed : null}
        />
      )}
    />
  );
}

function ClubCells({ club: c }: { club: ClubSummary }) {
  return (
    <>
      <td className="col-name" title={c.name}>
        <WithPicture picture={<ClubLogo uid={c.uid} name={c.name} size={PICTURE_SIZE.clubRowLogo} />}>
          <div className="name">
            <span className="name-text">{c.name}</span>
            {c.women && <span className="tag women">{clubListText.women}</span>}
          </div>
          {c.shortName && c.shortName !== c.name && <div className="sub">{c.shortName}</div>}
        </WithPicture>
      </td>
      <td className="num col-rep">
        <ReputationMeter value={c.reputation} />
      </td>
      {/* A year, so it is printed as it is: no thousands separator. */}
      <td className="num date col-founded">{c.founded ?? profileText.none}</td>
      <td className="num uid col-uid">{c.uid}</td>
    </>
  );
}
