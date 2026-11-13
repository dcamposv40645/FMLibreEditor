import { useState } from "react";
import { Ability } from "./Ability";
import { formatDate } from "./format";
import { jobLabel, profileText, staffListText } from "./labels";
import { ListBrowser, type Followed } from "./ListBrowser";
import { MassEditButton } from "./MassEditDialog";
import { ClubLogo, NationFlag, PICTURE_SIZE, WithPicture } from "./Picture";
import { restored, useSnapshot } from "./restore";
import { SavedFilterControls } from "./SavedFilterControls";
import { readStaffOrder, STAFF_FILTERS } from "./savedFilters";
import { SortHeader } from "./SortHeader";
import { countStaffFilters, readStaffFields, StaffFilterBar, toStaffFilter } from "./StaffFilters";
import { employerOf, HumanBadge, StaffProfile } from "./StaffProfile";
import type { StaffOrder, StaffSort, StaffSummary } from "./types";

const text = staffListText;
const columns = text.columns;

/** The order the headers show until one is clicked: the best first, as the search has them (the manager first). */
const DEFAULT_ORDER: StaffOrder = { by: "currentAbility", ascending: false };

/** The columns whose first click sorts from A to Z; the numbers start with the highest. */
const FIRST_ASCENDING: ReadonlySet<StaffSort> = new Set<StaffSort>(["name", "nation", "club"]);

interface StaffBrowserProps {
  version: number;
  active: boolean;
  /** Whether the game's layout tells women from men, for the filter that keeps to one of them. */
  genderKnown: boolean;
  /** The member of staff FM's screen shows, to open (see followView.ts). */
  followed?: Followed<StaffSummary> | null;
}

/** The staff table with the profile of the selected member of staff beside it. */
export function StaffBrowser({ version, active, genderKnown, followed }: StaffBrowserProps) {
  // The filters and the sort start as they were before the language was switched, if it was (see `restore.ts`).
  const [fields, setFields] = useState(() => restored("staffFields", readStaffFields));
  // Null until a header is clicked: no order is asked for, and the search answers with the human manager first.
  const [order, setOrder] = useState<StaffOrder | null>(() => restored("staffOrder", readStaffOrder));
  useSnapshot("staffFields", () => fields);
  useSnapshot("staffOrder", () => order);
  // Where the layout cannot tell women from men, the bar offers no such choice, and a saved filter that made one is
  // taken without it.
  const filter = toStaffFilter(genderKnown ? fields : { ...fields, gender: "" });
  const filterCount = countStaffFilters(filter);
  const sorted = order ?? DEFAULT_ORDER;
  const sortBy = (by: StaffSort) =>
    setOrder({ by, ascending: sorted.by === by ? !sorted.ascending : FIRST_ASCENDING.has(by) });
  const sortHeader = (by: StaffSort, className: string) => (
    <SortHeader
      by={by}
      order={sorted}
      onSort={sortBy}
      label={columns[by]}
      title={text.sortBy(text.sortNames[by])}
      className={className}
    />
  );

  return (
    <ListBrowser<StaffSummary>
      name="staff"
      command="search_staff"
      requestArgs={{ filter, order: order ?? undefined }}
      version={version}
      active={active}
      placeholder={text.search}
      summary={(total, shown) =>
        text.summary(total, shown, order && { by: text.sortNames[order.by], ascending: order.ascending })
      }
      empty={(query) => text.empty(query, filterCount > 0)}
      filterBar={
        <StaffFilterBar
          fields={fields}
          onChange={setFields}
          active={filterCount}
          genderKnown={genderKnown}
          actions={
            <SavedFilterControls
              kind={STAFF_FILTERS}
              fields={fields}
              order={order}
              filtered={filterCount > 0}
              onApply={(savedFields, savedOrder) => {
                setFields(savedFields);
                setOrder(savedOrder);
              }}
            />
          }
        />
      }
      toolbarActions={(list) => <MassEditButton entity="staff" filters={{ staffFilter: filter }} list={list} />}
      tableClass="staff-grid"
      header={
        <>
          {sortHeader("name", "col-name")}
          {sortHeader("age", "num col-age")}
          <th className="col-job">{columns.job}</th>
          {sortHeader("club", "col-club")}
          {sortHeader("nation", "col-nation")}
          <th className="col-born">{columns.born}</th>
          {sortHeader("currentAbility", "num col-ca")}
          {sortHeader("potentialAbility", "num col-pa")}
          <th className="num col-uid">{columns.id}</th>
        </>
      }
      cells={(s) => <StaffCells staff={s} />}
      followed={followed}
      profile={(s, panel) => <StaffProfile member={s} version={panel.version} onClose={panel.onClose} />}
    />
  );
}

function StaffCells({ staff: s }: { staff: StaffSummary }) {
  return (
    <>
      <td className="col-name">
        <div className="name">
          {s.name}
          {s.isHuman && <HumanBadge />}
        </div>
        {s.fullName && s.fullName !== s.name && <div className="sub">{s.fullName}</div>}
      </td>
      <td className="num col-age">{s.age ?? profileText.none}</td>
      {/* A long job or club is cut short with an ellipsis (see App.css); the hover text has all of it. */}
      <td className="col-job" title={s.job === null ? undefined : jobLabel(s.job)}>
        {jobLabel(s.job)}
      </td>
      <td className="col-club" title={s.club ?? undefined}>
        <WithPicture picture={<ClubLogo uid={s.clubUid} name={s.club} size={PICTURE_SIZE.rowLogo} />}>
          {employerOf(s) ?? <span className="muted">{profileText.none}</span>}
        </WithPicture>
      </td>
      <td className="col-nation">
        <WithPicture picture={<NationFlag uid={s.nationUid} />}>{s.nation ?? profileText.none}</WithPicture>
      </td>
      <td className="date col-born">{formatDate(s.dateOfBirth)}</td>
      <td className="num col-ca">
        <Ability value={s.currentAbility} />
      </td>
      <td className="num col-pa">
        <Ability value={s.potentialAbility} />
      </td>
      <td className="num uid col-uid">{s.uid}</td>
    </>
  );
}
