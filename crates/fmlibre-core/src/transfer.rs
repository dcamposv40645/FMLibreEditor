//! Moving a player to another club.
//!
//! Each team keeps a list of player pointers: the players under contract with it (those
//! out on loan too) and the players on loan to it. A player points at the team he plays
//! for, and his contract at the team that employs him. A move points both at the new
//! club's senior team, adds the player to that team's list and takes him out of his old
//! club's lists.
//!
//! The player goes into the senior team's list where its block has room: below its capacity,
//! or up to the end of the allocator's block (see [`HeapLayout`]). After a load every list is
//! full; a full list is then grown into memory of the editor's own, as the lists of ties are
//! (see `own_memory.rs`). Where the editor cannot grow it, a full list is no obstacle only when
//! one of the team's players is released in the same move: the player who joins takes his
//! slot, as in a swap (see [`Session::plan_club_move_releasing`]). Only players with simple
//! ties move: under contract with the club they play for (no loans), and not player-coaches.
//!
//! A free agent, who has no club and no contract, joins a club under a contract the editor makes
//! for him (see `signing.rs`).
//!
//! A player who leaves a club gives up the captaincies he holds in its teams: they are left
//! empty, as a youth team's often are, for the club to fill. One who leaves a club a human
//! manager runs also leaves his places in its selections empty, as he does when the game
//! moves him (see [`crate::selection`]); where the editor cannot find them, he is not moved.
//!
//! A release takes him out of his club's lists and leaves him with no team and no contract,
//! as the game's own free agents have. His contract is left as it was: nothing else in the
//! game points at it.
//!
//! Every write is a [`Patch`] holding the bytes it expects and the bytes it leaves, so a
//! move only lands on the memory it was planned from, and is only undone while the game
//! still holds what it wrote.

use std::fmt;

use fmlibre_mem::{Process, is_user_pointer};
use serde::{Deserialize, Serialize};

use crate::clubs::ClubSummary;
use crate::coded::Coded;
use crate::date::FmDate;
use crate::edit::{EditError, Entity, Field, Kind, Target};
use crate::health::Lock;
use crate::own_memory::Growth;
use crate::players::{PlayerIndex, PlayerSummary};
use crate::profile::HeapLayout;
use crate::selection::Leaver;
use crate::session::Session;
use crate::signing::NewContract;
use crate::staff::job_name;

/// The job code of a player's contract.
pub(crate) const PLAYER_JOB: u8 = 1;

/// The squad number of a player without one.
pub(crate) const NO_SQUAD_NUMBER: u8 = 255;

/// The listing bits of a contract: on the transfer list and on the loan list.
pub(crate) const LISTED: u8 = 0b11;

/// The longest list the editor reads; no squad comes near it.
const LONGEST_LIST: u64 = 2_000;

/// Bytes the editor writes at one address, with the bytes it expects to find there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Patch {
    pub(crate) address: u64,
    pub(crate) before: Vec<u8>,
    pub(crate) after: Vec<u8>,
}

/// The contract a move gives the player, as stored: a wage in pounds per week and an end
/// date as a number like 20290630. What is left out stays as the contract has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveTerms {
    pub wage: Option<i64>,
    pub expires: Option<i64>,
}

/// A move worked out from the game as it is now, ready to be written: one player to
/// another club, two players swapping clubs, a player released from his club, or a free agent
/// signed.
#[derive(Debug, Clone)]
pub struct MovePlan {
    pub player: u32,
    /// The club he leaves; 0 for a free agent, who has none.
    pub from_club: u32,
    /// The club he joins; 0 for a release, after which he has none.
    pub to_club: u32,
    /// In a swap, the player who goes the other way.
    pub other: Option<u32>,
    /// The player of the club he joins who is released to make room for him, if any.
    pub released: Option<u32>,
    /// The writes, in the order they are made.
    pub(crate) patches: Vec<Patch>,
    /// Those who leave a club a human manager runs: their places in its selections are emptied too, found as the move
    /// is written (see [`crate::selection`]).
    pub(crate) leavers: Vec<Leaver>,
    /// The squad lists that have no room for him, grown into memory of the editor's own as the move is written (see
    /// `own_memory.rs`).
    pub(crate) grow: Vec<Growth>,
    /// For a free agent, the contract the editor makes for him (see `signing.rs`).
    pub contract: Option<NewContract>,
}

impl MovePlan {
    /// How many writes the move takes, besides those to selections.
    pub fn writes(&self) -> usize {
        self.patches.len() + self.grow.len()
    }

    /// How many squad lists the move grows.
    pub fn grows(&self) -> usize {
        self.grow.len()
    }

    /// Whether the move takes a player out of a club a human manager runs, which takes a few seconds more to write.
    pub fn leaves_a_human_club(&self) -> bool {
        !self.leavers.is_empty()
    }
}

/// A move that was written: what undoing it takes.
#[derive(Debug, Clone)]
pub struct ClubMove {
    pub player: u32,
    pub from_club: u32,
    /// 0 for a release.
    pub to_club: u32,
    /// In a swap, the player who went the other way.
    pub other: Option<u32>,
    /// The player released to make room for him, if any.
    pub released: Option<u32>,
    /// How many squad lists it grew into memory of the editor's own.
    pub grew: usize,
    patches: Vec<Patch>,
    /// The selections of a club a human manager runs whose slots the move emptied, one write each.
    selections: Vec<Patch>,
}

impl ClubMove {
    /// How many selections of a club a human manager runs the move took a player out of.
    pub fn selections(&self) -> usize {
        self.selections.len()
    }
}

/// A player of a club's senior team whom a move to the club can release to make room (see
/// [`Session::room_makers`]), with what makes him a worse choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMaker {
    pub uid: u32,
    /// He has a current injury.
    pub injured: bool,
    /// He captains one of the club's teams, or is its vice-captain.
    pub captain: bool,
}

/// What ties a player to his club, checked: the player, the team he plays for, his
/// contract and the team that employs him, the club, and every list of it that holds him.
struct Departure {
    uid: u32,
    object: u64,
    team: u64,
    contract: u64,
    employer: u64,
    /// The contract's bytes, as read.
    record: Vec<u8>,
    club: u64,
    club_uid: u32,
    /// Whether a human manager runs the club.
    human: bool,
    /// The club's teams.
    teams: Vec<u64>,
    /// Each list of the club that holds him: the team, the list, where in it he is, and what it holds.
    leaving: Vec<(u64, PointerList, usize, Vec<u64>)>,
}

/// The senior team a player joins, and its list.
struct Arrival {
    club_uid: u32,
    senior: u64,
    list: PointerList,
    elements: Vec<u64>,
}

#[derive(Debug)]
pub enum MoveError {
    /// The player's object no longer carries his UID: the game has moved on.
    Moved {
        uid: u32,
    },
    /// The club no longer carries its UID, or has no senior team that points back at it.
    NotAClub {
        uid: u32,
    },
    /// The player has no club.
    FreeAgent,
    /// The player is at that club already.
    SameClub,
    /// The player is on loan, to his club or from it.
    OnLoan,
    /// Making a loan permanent asked for a player who is not on loan.
    NotOnLoan,
    /// A player-coach, who works as staff too.
    PlayerCoach,
    /// The contract does not point back at the player, is not a player's, or names two teams.
    UnusualContract,
    /// The player is at a club a human manager runs, and the editor cannot find its selections in this version of the
    /// game.
    HumanClub,
    /// One club is a men's club and the other a women's club.
    MenAndWomen,
    /// The player is not in his team's list once, or is in the new team's list already.
    NotListed,
    /// The new club's senior team has no room in its list for one more player.
    NoRoom {
        players: u64,
    },
    /// Healing asked for a player who has no current injury.
    NotInjured,
    /// Lifting bans asked for a player who has none.
    NotBanned,
    /// The player has no tie at this place in his list of relations, or it is not one the editor changes.
    NoRelation {
        index: usize,
    },
    /// The player's list of relations has no room for another.
    RelationsFull,
    /// The new nation or club is not one of the game's own, or not of the kind the tie takes.
    WrongTarget,
    /// The player has that tie already.
    SameRelation,
    /// A homegrown rule the editor does not write.
    UnknownRule {
        rule: u8,
    },
    /// The club has no team at this place in its list of teams, or it does not check out.
    NoTeam {
        index: usize,
    },
    /// A captain asked for who does not play for the team.
    NotInTeam,
    /// A contract term the editor does not write, such as an end date that has passed.
    OutOfRange {
        field: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },
    /// Memory at `address` is not what the move expects there: the game has changed it.
    Changed {
        address: u64,
    },
    /// The bytes read back after writing are not the ones written.
    NotApplied {
        address: u64,
    },
    /// The member of staff is the human manager.
    You,
    /// The member of staff has no contract.
    Unemployed,
    /// The club or one of its teams points at the member of staff directly, other than in its places for a director of
    /// football and a manager, or someone with another job is in one of those places.
    StaffRole,
    /// None of the club's department lists holds the member of staff, or the new club's holds him already.
    NotInDepartment,
    /// The club's list of a department has no room for one more.
    DepartmentFull {
        department: &'static str,
        staff: u64,
    },
    /// A job the editor does not give or take away (the board's, one it cannot name), or one whose place the layout
    /// does not put.
    UnusualJob {
        job: u8,
    },
    /// The club has a director of football already: the member of staff with this id.
    DirectorTaken {
        uid: u32,
    },
    /// The club's senior team has a manager already: the member of staff with this id.
    ManagerTaken {
        uid: u32,
    },
    /// The club's senior team's manager is the human manager.
    YourClub,
    /// A member of staff who has no club is signed for a job, and none was given.
    NoJob,
    /// The club's senior team plays at no stadium the editor can read.
    NoStadium,
    /// Other objects of the game hold the stadium's name too: changed in place, it would change for them as well.
    SharedName {
        holders: u32,
    },
    /// The new name does not fit the block the game gave the old one: it must take `min` to `max` bytes of UTF-8.
    NameLength {
        min: usize,
        max: usize,
        bytes: usize,
    },
    /// The new name is empty, has spaces at its ends, or holds a control character.
    BadName,
    /// The editor cannot find the retirements the game has planned in this build, or they do not check out.
    NoRetirements,
    /// Keeping a player from retiring asked for one the game has not planned to retire who already has the traits.
    NotRetiring,
    /// The player has no contract.
    NoContract,
    /// The contract has no clause at this place in its list.
    NoClause {
        index: usize,
    },
    /// The clause has no amount or condition to change (`clause.amount` or `clause.condition`).
    NotInClause {
        field: &'static str,
    },
    /// The clause would be as it is, or the contract has a release clause already.
    SameClause,
    /// The contract's list of clauses has no room for another.
    ClausesFull,
    /// The editor cannot find the game's pool of contracts in this build, or it does not check out (see
    /// `object_pool.rs`).
    NoPool,
    /// The game's pool of contracts has no free slot: the game takes more room only when it makes a contract itself.
    PoolFull,
    /// The club has no player under a plain contract with its senior team whose contract a new one can be modelled on.
    NoModel,
    /// The club has no member of staff under a full contract of his own with it whose contract a new one for a member
    /// of staff can be modelled on: of the `staff` its departments and its director of football's place hold, none; and
    /// none at all at a club of a league the save does not play, where the game keeps no staff.
    NoStaffModel {
        staff: usize,
    },
    /// The editor cannot read the in-game date, which a new contract starts on.
    NoDate,
    /// The layout does not say what starting a loan takes in this game (see [`crate::profile::LoanStartLayout`]).
    NoLoanStart,
    /// The game holds no loan the editor can model a new one on: none of its loan contracts is a running loan with no
    /// option to buy.
    NoLoanModel,
    /// The editor does not make this change to this game: see [`Lock`].
    Locked(Lock),
    Memory(fmlibre_mem::Error),
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoveError::Moved { uid } => {
                write!(f, "the player with UID {uid} has moved in the game's memory; reload and try again")
            }
            MoveError::NotAClub { uid } => write!(f, "the club with UID {uid} has no senior team to join"),
            MoveError::FreeAgent => f.write_str("the player is a free agent, with no club to leave or swap"),
            MoveError::SameClub => f.write_str("the player is at this club already"),
            MoveError::OnLoan => f.write_str(
                "the player is on loan: recall him from his loan first, or make the loan permanent, in the Loan part of \n                 his profile",
            ),
            MoveError::NotOnLoan => f.write_str("the player is not on loan"),
            MoveError::PlayerCoach => f.write_str("player-coaches cannot be moved: they work as staff too"),
            MoveError::UnusualContract => f.write_str("the player's contract is not a plain player's contract"),
            MoveError::HumanClub => f.write_str(
                "the player is at a club a human manager runs, and the editor cannot yet find that club's team \
                 selections in this version of the game to take him out of them; he leaves it in the game itself",
            ),
            MoveError::MenAndWomen => f.write_str("one club is a men's club and the other a women's club"),
            MoveError::NotListed => f.write_str("the squad lists do not hold the player where they should"),
            MoveError::NoRoom { players } => write!(
                f,
                "the club's squad list has no room for another player ({players} now), and the editor cannot \
                 make it bigger here"
            ),
            MoveError::NotInjured => f.write_str("the player has no injury to heal"),
            MoveError::NotBanned => f.write_str("the player has no ban to lift"),
            MoveError::NoRelation { index } => {
                write!(f, "the player has no nationality or homegrown tie number {index}")
            }
            MoveError::RelationsFull => f.write_str(
                "the player's list of nationalities and ties has no room for another, and the editor cannot make it \
                 bigger here; change one of them instead",
            ),
            MoveError::WrongTarget => {
                f.write_str("that is not a nation or club of the game, of the kind this tie takes")
            }
            MoveError::SameRelation => f.write_str("the player has that already"),
            MoveError::UnknownRule { rule } => write!(f, "homegrown rule {rule} is not one the editor writes"),
            MoveError::NoTeam { index } => write!(f, "the club has no team number {index} that checks out"),
            MoveError::NotInTeam => f.write_str("the player does not play for this team"),
            MoveError::OutOfRange { field, value, min, max } => {
                write!(f, "{value} is not a valid {field}; it takes {min} to {max}")
            }
            MoveError::Changed { address } => {
                write!(f, "the game has changed the memory at {address:#x} since; reload and try again")
            }
            MoveError::NotApplied { address } => write!(f, "wrote to {address:#x} but read back something else"),
            MoveError::You => f.write_str("this is you, the human manager"),
            MoveError::Unemployed => f.write_str("the member of staff has no club"),
            MoveError::StaffRole => f.write_str(
                "the club or one of its teams points at this member of staff directly, as a club does at a manager \
                 who is also its director of football; the game moves him itself",
            ),
            MoveError::NotInDepartment => f.write_str("the club's staff lists do not hold him where they should"),
            MoveError::DepartmentFull { department, staff } => write!(
                f,
                "the club's {department} staff list has no room for another ({staff} now), and the editor cannot \
                 make it bigger here"
            ),
            MoveError::UnusualJob { job } => match job_name(*job) {
                Some(name) => write!(f, "the editor does not yet move staff whose job is {name}, or give that job"),
                None => write!(f, "the editor does not yet move staff whose job is {job}, or give that job"),
            },
            MoveError::DirectorTaken { uid } => write!(
                f,
                "the club has a director of football already (the member of staff with id {uid}); give this one \
                 another job, or move that one first"
            ),
            MoveError::ManagerTaken { uid } => write!(
                f,
                "the club has a manager already (the member of staff with id {uid}); give this one another job, or \
                 move that one first"
            ),
            MoveError::YourClub => {
                f.write_str("you manage this club; give this member of staff another job there, the editor never takes your place")
            }
            MoveError::NoJob => f.write_str("choose the job the member of staff takes at the club"),
            MoveError::NoStadium => f.write_str("the club's first team plays at no stadium the editor can read"),
            MoveError::SharedName { holders } => write!(
                f,
                "{holders} things in the game share this stadium's name, so it cannot be changed for the stadium alone"
            ),
            MoveError::NameLength { min, max, bytes } => write!(
                f,
                "a new name for this stadium must take {min} to {max} bytes, to fit where the game keeps its name; \
                 this one takes {bytes}"
            ),
            MoveError::BadName => {
                f.write_str("a stadium's name cannot be empty, start or end with a space, or hold control characters")
            }
            MoveError::NoRetirements => {
                f.write_str("the editor cannot find the retirements the game has planned in this version of the game")
            }
            MoveError::NotRetiring => {
                f.write_str("the game has not planned the player's retirement, and he already means to play on")
            }
            MoveError::NoContract => f.write_str("the player has no contract"),
            MoveError::NoClause { index } => write!(f, "the contract has no clause number {index}"),
            MoveError::NotInClause { field } => write!(
                f,
                "this clause has no {} to change",
                if *field == "clause.amount" { "amount" } else { "condition" }
            ),
            MoveError::SameClause => f.write_str("the contract has that already"),
            MoveError::ClausesFull => f.write_str(
                "the contract's list of clauses has no room for another, and the editor cannot make it bigger",
            ),
            MoveError::NoPool => f.write_str(
                "the editor cannot find where the game keeps its contracts in this version of the game, so it cannot \
                 give a free agent one",
            ),
            MoveError::PoolFull => f.write_str(
                "the game has no room for another contract until it makes one itself, as it does when the days go \
                 by: continue in the game for a day, then try again",
            ),
            MoveError::NoModel => f.write_str(
                "the club has no player under a plain contract with its first team to model the new contract on",
            ),
            MoveError::NoStaffModel { staff: 0 } => f.write_str(
                "this club has no staff at all: the game keeps none at clubs of leagues that are not playable in this \
                 save",
            ),
            MoveError::NoStaffModel { .. } => {
                f.write_str("none of this club's staff has a full contract of their own to model a new one on")
            }
            MoveError::NoDate => f.write_str("the editor cannot read the in-game date, which a new contract starts on"),
            MoveError::NoLoanStart => {
                f.write_str("the editor does not know how to start a loan in this version of the game yet")
            }
            MoveError::NoLoanModel => f.write_str(
                "the game holds no running loan without an option to buy for the editor to model a new one on",
            ),
            MoveError::Locked(lock) => write!(f, "nothing was changed: {lock}"),
            MoveError::Memory(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MoveError {}

impl Coded for MoveError {
    fn code(&self) -> &'static str {
        match self {
            MoveError::Moved { .. } => "move.moved",
            MoveError::NotAClub { .. } => "move.notAClub",
            MoveError::FreeAgent => "move.freeAgent",
            MoveError::SameClub => "move.sameClub",
            MoveError::OnLoan => "move.onLoan",
            MoveError::NotOnLoan => "move.notOnLoan",
            MoveError::PlayerCoach => "move.playerCoach",
            MoveError::UnusualContract => "move.unusualContract",
            MoveError::HumanClub => "move.humanClub",
            MoveError::MenAndWomen => "move.menAndWomen",
            MoveError::NotListed => "move.notListed",
            MoveError::NoRoom { .. } => "move.noRoom",
            MoveError::NotInjured => "heal.notInjured",
            MoveError::NotBanned => "ban.notBanned",
            MoveError::NoRelation { .. } => "relation.missing",
            MoveError::RelationsFull => "relation.noRoom",
            MoveError::WrongTarget => "relation.target",
            MoveError::SameRelation => "relation.same",
            MoveError::UnknownRule { .. } => "relation.rule",
            MoveError::NoTeam { .. } => "squad.noTeam",
            MoveError::NotInTeam => "captain.notInTeam",
            MoveError::OutOfRange { .. } => "move.outOfRange",
            MoveError::Changed { .. } => "move.changed",
            MoveError::NotApplied { .. } => "move.notApplied",
            MoveError::You => "staff.you",
            MoveError::Unemployed => "staff.unemployed",
            MoveError::StaffRole => "staff.role",
            MoveError::NotInDepartment => "staff.notListed",
            MoveError::DepartmentFull { .. } => "staff.noRoom",
            MoveError::UnusualJob { .. } => "staff.job",
            MoveError::DirectorTaken { .. } => "staff.directorTaken",
            MoveError::ManagerTaken { .. } => "staff.managerTaken",
            MoveError::YourClub => "staff.yourClub",
            MoveError::NoJob => "staff.noJob",
            MoveError::NoStadium => "stadium.none",
            MoveError::SharedName { .. } => "stadium.shared",
            MoveError::NameLength { .. } => "stadium.nameLength",
            MoveError::BadName => "stadium.badName",
            MoveError::NoRetirements => "retirement.unknown",
            MoveError::NotRetiring => "retirement.none",
            MoveError::NoContract => "clause.noContract",
            MoveError::NoClause { .. } => "clause.missing",
            MoveError::NotInClause { .. } => "clause.notInClause",
            MoveError::SameClause => "clause.same",
            MoveError::ClausesFull => "clause.noRoom",
            MoveError::NoPool => "sign.noPool",
            MoveError::PoolFull => "sign.poolFull",
            MoveError::NoModel => "sign.noModel",
            MoveError::NoStaffModel { .. } => "staff.noModel",
            MoveError::NoDate => "sign.noDate",
            MoveError::NoLoanStart => "loan.notMapped",
            MoveError::NoLoanModel => "loan.noModel",
            MoveError::Locked(lock) => lock.code(),
            MoveError::Memory(err) => err.code(),
        }
    }

    fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            MoveError::Moved { uid }
            | MoveError::NotAClub { uid }
            | MoveError::DirectorTaken { uid }
            | MoveError::ManagerTaken { uid } => {
                vec![("uid", uid.to_string())]
            }
            MoveError::NoRoom { players } => vec![("players", players.to_string())],
            MoveError::NoTeam { index } => vec![("index", index.to_string())],
            MoveError::OutOfRange { field, value, min, max } => vec![
                ("field", (*field).to_owned()),
                ("value", value.to_string()),
                ("min", min.to_string()),
                ("max", max.to_string()),
            ],
            MoveError::Changed { address } | MoveError::NotApplied { address } => {
                vec![("address", format!("{address:#x}"))]
            }
            MoveError::DepartmentFull { department, staff } => {
                vec![("department", (*department).to_owned()), ("staff", staff.to_string())]
            }
            MoveError::UnusualJob { job } => vec![("job", job.to_string())],
            MoveError::NoStaffModel { staff } => vec![("staff", staff.to_string())],
            MoveError::SharedName { holders } => vec![("holders", holders.to_string())],
            MoveError::NameLength { min, max, bytes } => {
                vec![("min", min.to_string()), ("max", max.to_string()), ("bytes", bytes.to_string())]
            }
            MoveError::NoRelation { index } | MoveError::NoClause { index } => vec![("index", index.to_string())],
            MoveError::NotInClause { field } => vec![("field", (*field).to_owned())],
            MoveError::UnknownRule { rule } => vec![("rule", rule.to_string())],
            MoveError::Locked(lock) => lock.params(),
            MoveError::Memory(err) => err.params(),
            _ => Vec::new(),
        }
    }
}

impl From<fmlibre_mem::Error> for MoveError {
    fn from(err: fmlibre_mem::Error) -> Self {
        MoveError::Memory(err)
    }
}

/// A list of pointers the game keeps as `(begin, end, capacity)` at `header`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PointerList {
    header: u64,
    begin: u64,
    end: u64,
    capacity: u64,
}

impl PointerList {
    /// The list whose header is at `header`, or `None` when the header does not hold one.
    pub(crate) fn read(process: &Process, header: u64) -> Result<Option<Self>, MoveError> {
        Self::read_up_to(process, header, LONGEST_LIST)
    }

    /// [`PointerList::read`], where a header of three zeros is a list with nothing in it and no buffer yet.
    pub(crate) fn read_or_empty(process: &Process, header: u64) -> Result<Option<Self>, MoveError> {
        if process.read_bytes(header, 24)?.iter().all(|&b| b == 0) {
            return Ok(Some(Self { header, begin: 0, end: 0, capacity: 0 }));
        }
        Self::read(process, header)
    }

    /// [`PointerList::read`] for a list whose block may hold up to `longest` pointers.
    pub(crate) fn read_up_to(process: &Process, header: u64, longest: u64) -> Result<Option<Self>, MoveError> {
        let bytes = process.read_bytes(header, 24)?;
        let word = |i: usize| u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().expect("8 bytes"));
        let (begin, end, capacity) = (word(0), word(1), word(2));
        let sane = is_user_pointer(begin)
            && begin <= end
            && end <= capacity
            && (end - begin).is_multiple_of(8)
            && (capacity - begin).is_multiple_of(8)
            && capacity - begin <= 8 * longest;
        Ok(sane.then_some(Self { header, begin, end, capacity }))
    }

    pub(crate) fn len(&self) -> u64 {
        (self.end - self.begin) / 8
    }

    /// Where its first pointer is.
    pub(crate) fn begin_address(&self) -> u64 {
        self.begin
    }

    /// Where it keeps its begin, end and capacity, and what they are.
    pub(crate) fn header(&self) -> u64 {
        self.header
    }

    pub(crate) fn words(&self) -> [u64; 3] {
        [self.begin, self.end, self.capacity]
    }

    pub(crate) fn elements(&self, process: &Process) -> Result<Vec<u64>, MoveError> {
        if self.end == self.begin {
            return Ok(Vec::new());
        }
        let bytes = process.read_bytes(self.begin, (self.end - self.begin) as usize)?;
        Ok(bytes.as_chunks::<8>().0.iter().map(|word| u64::from_le_bytes(*word)).collect())
    }

    /// How many pointers its block holds: the capacity, or the whole block of the
    /// allocator when its size is known and nothing has been written past the capacity.
    pub(crate) fn room(&self, process: &Process, heap: &HeapLayout) -> Result<u64, MoveError> {
        if self.begin == 0 {
            return Ok(0);
        }
        let capacity = (self.capacity - self.begin) / 8;
        let Some(block) = heap.block_size(self.capacity - self.begin) else {
            return Ok(capacity);
        };
        let spare = process.read_bytes(self.capacity, (self.begin + block - self.capacity) as usize)?;
        Ok(if spare.iter().all(|&b| b == 0) { block / 8 } else { capacity })
    }
}

/// Out of `list`, where the player is at `index`: the players after him move up one, the
/// list ends one earlier, and the slot it gave up is cleared. On the way the list repeats its
/// last player for a moment; it never holds an empty slot.
pub(crate) fn removal_patches(list: &PointerList, index: usize, elements: &[u64], patches: &mut Patches) {
    let last = elements.len() - 1;
    let bytes = |range: &[u64]| range.iter().flat_map(|e| e.to_le_bytes()).collect::<Vec<u8>>();
    patches.push(list.begin + index as u64 * 8, bytes(&elements[index..last]), bytes(&elements[index + 1..]));
    patches.push(list.header + 8, list.end.to_le_bytes(), (list.end - 8).to_le_bytes());
    patches.push(list.begin + last as u64 * 8, elements[last].to_le_bytes(), 0u64.to_le_bytes());
}

/// Collects the patches of a move, leaving out the ones that would change nothing.
#[derive(Default)]
pub(crate) struct Patches(pub(crate) Vec<Patch>);

impl Patches {
    pub(crate) fn push(&mut self, address: u64, before: impl Into<Vec<u8>>, after: impl Into<Vec<u8>>) {
        let (before, after) = (before.into(), after.into());
        if before != after {
            self.0.push(Patch { address, before, after });
        }
    }
}

/// `found`, each with his current ability, in the order to suggest them: those neither injured nor a captain first,
/// the weakest first among each; a tie keeps the order of the list.
fn suggestion_order(mut found: Vec<(RoomMaker, u16)>) -> Vec<RoomMaker> {
    found.sort_by_key(|&(maker, ability)| (maker.injured || maker.captain, ability));
    found.into_iter().map(|(maker, _)| maker).collect()
}

impl Session {
    /// Works out moving `player` to the senior team of `club` from the game as it is now,
    /// under the contract `terms`. `human_clubs` are the UIDs of the clubs human managers
    /// run: a player who leaves one also leaves its selections. A free agent is signed under
    /// a contract the editor makes (see [`Session::plan_signing`]). Nothing is written.
    pub fn plan_club_move(
        &self,
        player: &PlayerSummary,
        club: &ClubSummary,
        terms: MoveTerms,
        human_clubs: &[u32],
    ) -> Result<MovePlan, MoveError> {
        if self.is_free_agent(player)? {
            return self.plan_signing(player, club, terms);
        }
        self.check_terms(terms, self.game_date())?;
        let leaving = self.departure(player, human_clubs)?;
        if self.process.read_u32(club.address + self.layout.club.uid)? != club.uid {
            return Err(MoveError::NotAClub { uid: club.uid });
        }
        let joining = self.arrival(club.address, &leaving)?;

        let mut patches = Patches::default();
        let grow = self.join_squad(&joining.list, &joining.elements, leaving.object, &mut patches)?;
        self.contract_patches(&leaving, joining.senior, terms, &mut patches);
        self.captaincy_patches(&leaving, &mut patches)?;
        for (_, list, index, elements) in &leaving.leaving {
            removal_patches(list, *index, elements, &mut patches);
        }
        Ok(MovePlan {
            player: leaving.uid,
            from_club: leaving.club_uid,
            to_club: joining.club_uid,
            other: None,
            released: None,
            patches: patches.0,
            leavers: self.leavers(&[&leaving])?,
            grow: grow.into_iter().collect(),
            contract: None,
        })
    }

    /// The player `object` joins `list`, which holds `elements`: in its first free slot where its block has room,
    /// or else in a list grown into memory of the editor's own (see `own_memory.rs`), returned for the plan to take.
    /// Where the editor cannot grow it (see [`Session::growth`]), a full list is refused.
    pub(crate) fn join_squad(
        &self,
        list: &PointerList,
        elements: &[u64],
        object: u64,
        patches: &mut Patches,
    ) -> Result<Option<Growth>, MoveError> {
        if list.len() < list.room(&self.process, &self.layout.heap)? {
            self.append_patches(list, object, patches)?;
            return Ok(None);
        }
        let bytes = elements.iter().chain([&object]).flat_map(|element| element.to_le_bytes()).collect();
        let growth = self.growth(list.header(), list.words(), bytes, 8);
        growth.map(Some).ok_or(MoveError::NoRoom { players: list.len() })
    }

    /// Works out moving `player` to the senior team of `club` under the contract `terms`, and
    /// releasing `leaver`, one of that team's players, to make room for him: the player takes
    /// his slot in the team's list, as in a swap, so a full list is no obstacle. `leaver` is
    /// released as [`Session::plan_release`] releases him. One plan, written and undone as
    /// one. Nothing is written.
    pub fn plan_club_move_releasing(
        &self,
        player: &PlayerSummary,
        club: &ClubSummary,
        terms: MoveTerms,
        leaver: &PlayerSummary,
        human_clubs: &[u32],
    ) -> Result<MovePlan, MoveError> {
        self.check_terms(terms, self.game_date())?;
        let leaving = self.departure(player, human_clubs)?;
        if self.process.read_u32(club.address + self.layout.club.uid)? != club.uid {
            return Err(MoveError::NotAClub { uid: club.uid });
        }
        let joining = self.arrival(club.address, &leaving)?;
        let released = self.departure(leaver, human_clubs)?;
        // His slot in the senior team's list is the one the player takes: releasing someone it does not hold makes no
        // room there.
        let slot = match joining.elements.iter().position(|&e| e == released.object) {
            Some(index) => joining.list.begin + index as u64 * 8,
            None => return Err(MoveError::NotListed),
        };

        let mut patches = Patches::default();
        // He leaves first, in the order a release goes, but where the senior team's list holds him the player comes
        // in instead.
        self.captaincy_patches(&released, &mut patches)?;
        patches.push(slot, released.object.to_le_bytes(), leaving.object.to_le_bytes());
        for (_, list, index, elements) in &released.leaving {
            if list.header != joining.list.header {
                removal_patches(list, *index, elements, &mut patches);
            }
        }
        self.free_agent_patches(&released, &mut patches);
        // Then the player, in the order a move goes.
        self.contract_patches(&leaving, joining.senior, terms, &mut patches);
        self.captaincy_patches(&leaving, &mut patches)?;
        for (_, list, index, elements) in &leaving.leaving {
            removal_patches(list, *index, elements, &mut patches);
        }
        Ok(MovePlan {
            player: leaving.uid,
            from_club: leaving.club_uid,
            to_club: joining.club_uid,
            other: None,
            released: Some(released.uid),
            patches: patches.0,
            leavers: self.leavers(&[&leaving, &released])?,
            grow: Vec::new(),
            contract: None,
        })
    }

    /// The players of `club`'s senior team whom a move to the club can release to make room
    /// (see [`Session::plan_club_move_releasing`]): those its list holds who could be released
    /// on their own, as the lists of `players` know them. Those to suggest come first: neither
    /// injured nor a captain, then the weakest.
    pub fn room_makers(
        &self,
        club: &ClubSummary,
        players: &PlayerIndex,
        human_clubs: &[u32],
    ) -> Result<Vec<RoomMaker>, MoveError> {
        let teams = self.club_teams(club, players)?;
        let Some(senior) = teams.first() else {
            return Ok(Vec::new());
        };
        let captains: Vec<u32> = teams.iter().flat_map(|team| [team.captain, team.vice_captain]).flatten().collect();
        let mut found = Vec::new();
        for member in &senior.players {
            let Some(player) = players.get(member.uid) else {
                continue;
            };
            // A loan in either direction is refused there too, but needs no reading to tell.
            if member.loan.is_some() || self.departure(player, human_clubs).is_err() {
                continue;
            }
            let maker =
                RoomMaker { uid: member.uid, injured: member.injuries > 0, captain: captains.contains(&member.uid) };
            found.push((maker, player.current_ability));
        }
        Ok(suggestion_order(found))
    }

    /// Works out releasing `player` from his club, which leaves him a free agent. He gives
    /// up its captaincies, leaves its lists, and then has no team and no contract. Nothing is
    /// written.
    pub fn plan_release(&self, player: &PlayerSummary, human_clubs: &[u32]) -> Result<MovePlan, MoveError> {
        let leaving = self.departure(player, human_clubs)?;
        let mut patches = Patches::default();
        self.captaincy_patches(&leaving, &mut patches)?;
        for (_, list, index, elements) in &leaving.leaving {
            removal_patches(list, *index, elements, &mut patches);
        }
        self.free_agent_patches(&leaving, &mut patches);
        Ok(MovePlan {
            player: leaving.uid,
            from_club: leaving.club_uid,
            to_club: 0,
            other: None,
            released: None,
            patches: patches.0,
            leavers: self.leavers(&[&leaving])?,
            grow: Vec::new(),
            contract: None,
        })
    }

    /// Works out making the loan of `player` permanent, on the contract `terms`: he stays where he plays, at the club
    /// he is on loan to, now under contract with its senior team from today, with the squad number and status he has
    /// there. He leaves every list of his own club and the captaincies he holds in it, and the loan is let go as the
    /// game lets one go when it ends: the record of his other contracts loses the loan contract, and when it holds
    /// nothing else he is left without one (see [`crate::loans`]). The loan contract and the record are left as they
    /// are, with nothing pointing at them. Nothing is written.
    pub fn plan_loan_permanent(
        &self,
        player: &PlayerSummary,
        terms: MoveTerms,
        human_clubs: &[u32],
    ) -> Result<MovePlan, MoveError> {
        self.check_terms(terms, self.game_date())?;
        let (process, layout) = (&self.process, &self.layout);
        let (p, c) = (&layout.player, &layout.contract);
        let loan_layout = layout.loan.as_ref().ok_or(MoveError::NotOnLoan)?;
        let object = player.address;
        if process.read_u32(object + p.at(p.uid, player.shift))? != player.uid {
            return Err(MoveError::Moved { uid: player.uid });
        }
        if player.shift != 0 {
            return Err(MoveError::PlayerCoach);
        }
        let loan = self.loan_contract(object, player.shift)?.ok_or(MoveError::NotOnLoan)?;
        let team = process.read_u64(object + p.team)?;
        let contract = process.read_u64(object + p.contract)?;
        if !is_user_pointer(team) || !is_user_pointer(contract) {
            return Err(MoveError::FreeAgent);
        }

        // His own contract must be his, a player's, with one team to its name; the loan contract must be with the team
        // he plays for, at another club.
        let record = process.read_bytes(contract, c.end() as usize)?;
        let loaned = process.read_bytes(loan, c.end() as usize)?;
        let word = |bytes: &[u8], offset: u64| {
            u64::from_le_bytes(bytes[offset as usize..offset as usize + 8].try_into().expect("8 bytes"))
        };
        let employer = word(&record, c.team);
        if word(&record, c.owner) != object + p.identity()
            || record[c.job as usize] != PLAYER_JOB
            || word(&record, c.senior_team) != employer
            || !is_user_pointer(employer)
        {
            return Err(MoveError::UnusualContract);
        }
        if word(&loaned, c.team) != team {
            return Err(MoveError::UnusualContract);
        }
        let new_club = process.read_u64(team + layout.team.club)?;
        let old_club = process.read_u64(employer + layout.team.club)?;
        if !is_user_pointer(new_club) || !is_user_pointer(old_club) || new_club == old_club {
            return Err(MoveError::NotOnLoan);
        }
        let new_uid = process.read_u32(new_club + layout.club.uid)?;
        let old_uid = process.read_u32(old_club + layout.club.uid)?;
        let human = human_clubs.contains(&old_uid);
        if human && !self.selections_known() {
            return Err(MoveError::HumanClub);
        }

        // Every list of his own club that holds him loses him. One does, as for any player out on loan: his employer's,
        // or for a youngster that of the youth team he played for (Mirza Ćatović, on loan from VfB Stuttgart, is in its
        // under-19s' list and not in the senior team's).
        let mut old_teams = match PointerList::read(process, old_club + layout.club.teams)? {
            Some(teams) => teams.elements(process)?,
            None => Vec::new(),
        };
        old_teams.sort_unstable();
        old_teams.dedup();
        let mut leaving = Vec::new();
        for &from in &old_teams {
            let Some(list) = PointerList::read(process, from + layout.team.players)? else {
                continue;
            };
            let elements = list.elements(process)?;
            let found: Vec<usize> = (0..elements.len()).filter(|&j| elements[j] == object).collect();
            match found[..] {
                [] => {}
                [index] => leaving.push((from, list, index, elements)),
                _ => return Err(MoveError::NotListed),
            }
        }
        if leaving.is_empty() {
            return Err(MoveError::NotListed);
        }
        // He stays in the list of the team he plays for, which holds him once.
        let Some(staying) = PointerList::read(process, team + layout.team.players)? else {
            return Err(MoveError::NotListed);
        };
        if staying.elements(process)?.iter().filter(|&&element| element == object).count() != 1 {
            return Err(MoveError::NotListed);
        }
        let (senior, _) = self.senior_team(new_club, new_uid)?;

        let mut patches = Patches::default();
        // His own contract: with the senior team of the club he stays at, from today, on the terms, with the squad
        // number and status he has there, and off the transfer and loan lists.
        for offset in [c.team, c.senior_team] {
            patches.push(contract + offset, employer.to_le_bytes(), senior.to_le_bytes());
        }
        let date_at = |offset: u64| &record[offset as usize..offset as usize + 4];
        if let Some(today) = self.game_date() {
            for offset in [c.signed, c.joined] {
                if Kind::Date.decode(date_at(offset)) != 0 {
                    patches.push(
                        contract + offset,
                        date_at(offset),
                        Kind::Date.encode(today.to_number(), date_at(offset)),
                    );
                }
            }
        }
        if let Some(wage) = terms.wage {
            let old = &record[c.wage as usize..c.wage as usize + 4];
            patches.push(contract + c.wage, old, Kind::I32.encode(wage, old));
        }
        if let Some(expires) = terms.expires {
            patches.push(contract + c.expires, date_at(c.expires), Kind::Date.encode(expires, date_at(c.expires)));
        }
        for offset in [c.squad_number, c.squad_status] {
            patches.push(contract + offset, [record[offset as usize]], [loaned[offset as usize]]);
        }
        let listing = record[c.listing as usize];
        patches.push(contract + c.listing, [listing], [listing & !LISTED]);
        // His own club: its captaincies he holds are left empty, and its lists lose him.
        let block = object + p.identity();
        for &from in &old_teams {
            for offset in [layout.team.captain, layout.team.vice_captain] {
                if process.read_u64(from + offset)? == block {
                    patches.push(from + offset, block.to_le_bytes(), 0u64.to_le_bytes());
                }
            }
        }
        for (_, list, index, elements) in &leaving {
            removal_patches(list, *index, elements, &mut patches);
        }
        // The loan, let go as the game lets one go.
        let at = object + p.at(loan_layout.record, player.shift);
        let held = process.read_u64(at)?;
        let mut others = Vec::new();
        for &offset in &loan_layout.others {
            others.push(process.read_u64(held + offset)?);
        }
        if others.iter().all(|&other| other == 0) {
            patches.push(at, held.to_le_bytes(), 0u64.to_le_bytes());
        } else {
            patches.push(held + loan_layout.contract, loan.to_le_bytes(), 0u64.to_le_bytes());
        }

        let leavers = if human { vec![self.leaver(object, &old_teams)?] } else { Vec::new() };
        Ok(MovePlan {
            player: player.uid,
            from_club: old_uid,
            to_club: new_uid,
            other: None,
            released: None,
            patches: patches.0,
            leavers,
            grow: Vec::new(),
            contract: None,
        })
    }

    /// The player `leaving` describes is left with no team and no contract. Only once no list
    /// holds him: the game may follow a listed player to his contract.
    fn free_agent_patches(&self, leaving: &Departure, patches: &mut Patches) {
        let p = &self.layout.player;
        patches.push(leaving.object + p.team, leaving.team.to_le_bytes(), 0u64.to_le_bytes());
        patches.push(leaving.object + p.contract, leaving.contract.to_le_bytes(), 0u64.to_le_bytes());
    }

    /// Works out `player` and `other` swapping clubs, each joining the other's senior team
    /// on his own contract. Each takes the other's slot in that team's list where it has him,
    /// so a full list is no obstacle then. Nothing is written.
    pub fn plan_club_swap(
        &self,
        player: &PlayerSummary,
        other: &PlayerSummary,
        human_clubs: &[u32],
    ) -> Result<MovePlan, MoveError> {
        let (first, second) = (self.departure(player, human_clubs)?, self.departure(other, human_clubs)?);
        let into_second = self.arrival(second.club, &first)?;
        let into_first = self.arrival(first.club, &second)?;

        let mut patches = Patches::default();
        // Each joins his new senior team in the slot of the player he swaps with, or in a free one, or a list grown
        // for him. The list whose slot changes hands is left out of the leaver's removals below.
        let mut kept: Vec<u64> = Vec::new();
        let mut grow = Vec::new();
        for (joining, joiner, leaver) in [(&into_second, &first, &second), (&into_first, &second, &first)] {
            match joining.elements.iter().position(|&e| e == leaver.object) {
                Some(index) => {
                    let slot = joining.list.begin + index as u64 * 8;
                    patches.push(slot, leaver.object.to_le_bytes(), joiner.object.to_le_bytes());
                    kept.push(joining.list.header);
                }
                None => grow.extend(self.join_squad(&joining.list, &joining.elements, joiner.object, &mut patches)?),
            }
        }
        self.contract_patches(&first, into_second.senior, MoveTerms::default(), &mut patches);
        self.contract_patches(&second, into_first.senior, MoveTerms::default(), &mut patches);
        for leaver in [&first, &second] {
            self.captaincy_patches(leaver, &mut patches)?;
            for (_, list, index, elements) in &leaver.leaving {
                if !kept.contains(&list.header) {
                    removal_patches(list, *index, elements, &mut patches);
                }
            }
        }
        Ok(MovePlan {
            player: first.uid,
            from_club: first.club_uid,
            to_club: second.club_uid,
            other: Some(second.uid),
            released: None,
            patches: patches.0,
            leavers: self.leavers(&[&first, &second])?,
            grow,
            contract: None,
        })
    }

    /// Checks what ties `player` to his club and reads every list of it that holds him.
    fn departure(&self, player: &PlayerSummary, human_clubs: &[u32]) -> Result<Departure, MoveError> {
        let (process, layout) = (&self.process, &self.layout);
        let (p, c) = (&layout.player, &layout.contract);
        let object = player.address;
        if process.read_u32(object + p.at(p.uid, player.shift))? != player.uid {
            return Err(MoveError::Moved { uid: player.uid });
        }
        if player.shift != 0 {
            return Err(MoveError::PlayerCoach);
        }
        let team = process.read_u64(object + p.team)?;
        let contract = process.read_u64(object + p.at(p.contract, player.shift))?;
        if !is_user_pointer(team) || !is_user_pointer(contract) {
            return Err(MoveError::FreeAgent);
        }

        // The contract must be this player's, a player's, with one team to its name.
        let record = process.read_bytes(contract, c.end() as usize)?;
        let word =
            |offset: u64| u64::from_le_bytes(record[offset as usize..offset as usize + 8].try_into().expect("8 bytes"));
        let byte = |offset: u64| record[offset as usize];
        let employer = word(c.team);
        if word(c.owner) != object + p.identity()
            || byte(c.job) != PLAYER_JOB
            || word(c.senior_team) != employer
            || !is_user_pointer(employer)
        {
            return Err(MoveError::UnusualContract);
        }
        // He may play for another team of the club that employs him (a youth team), not another club's.
        let old_club = process.read_u64(team + layout.team.club)?;
        if !is_user_pointer(old_club) || process.read_u64(employer + layout.team.club)? != old_club {
            return Err(MoveError::OnLoan);
        }
        let club_uid = process.read_u32(old_club + layout.club.uid)?;
        let human = human_clubs.contains(&club_uid);
        if human && !self.selections_known() {
            return Err(MoveError::HumanClub);
        }

        // He must be in the list of the team he plays for, once. Every list of his club that holds him loses him:
        // his employer's often does too, and a youngster may be listed with more than one youth side.
        let mut old_teams = match PointerList::read(process, old_club + layout.club.teams)? {
            Some(teams) => teams.elements(process)?,
            None => Vec::new(),
        };
        old_teams.sort_unstable();
        old_teams.dedup();
        if !old_teams.contains(&team) || !old_teams.contains(&employer) {
            return Err(MoveError::NotListed);
        }
        let mut leaving = Vec::new();
        for &from in &old_teams {
            // A team without a list holds nobody.
            let Some(list) = PointerList::read(process, from + layout.team.players)? else {
                continue;
            };
            let elements = list.elements(process)?;
            let found: Vec<usize> = (0..elements.len()).filter(|&j| elements[j] == object).collect();
            match found[..] {
                [] => {}
                [index] => leaving.push((from, list, index, elements)),
                _ => return Err(MoveError::NotListed),
            }
        }
        if !leaving.iter().any(|&(from, ..)| from == team) {
            return Err(MoveError::NotListed);
        }
        Ok(Departure {
            uid: player.uid,
            object,
            team,
            contract,
            employer,
            record,
            club: old_club,
            club_uid,
            human,
            teams: old_teams,
            leaving,
        })
    }

    /// Those of `departures` who leave a club a human manager runs, as far as its selections go.
    fn leavers(&self, departures: &[&Departure]) -> Result<Vec<Leaver>, MoveError> {
        departures.iter().filter(|d| d.human).map(|d| self.leaver(d.object, &d.teams)).collect()
    }

    /// Each captaincy the player `leaving` describes holds in his club's teams is left empty.
    fn captaincy_patches(&self, leaving: &Departure, patches: &mut Patches) -> Result<(), MoveError> {
        let t = &self.layout.team;
        let block = leaving.object + self.layout.player.identity();
        for &team in &leaving.teams {
            for offset in [t.captain, t.vice_captain] {
                if self.process.read_u64(team + offset)? == block {
                    patches.push(team + offset, block.to_le_bytes(), 0u64.to_le_bytes());
                }
            }
        }
        Ok(())
    }

    /// Checks the senior team of the club at `club` for the player `leaving` describes, and
    /// reads its list.
    fn arrival(&self, club: u64, leaving: &Departure) -> Result<Arrival, MoveError> {
        let (process, layout) = (&self.process, &self.layout);
        let club_uid = process.read_u32(club + layout.club.uid)?;
        if club == leaving.club {
            return Err(MoveError::SameClub);
        }
        let women = |address: u64| process.read_bytes(address + layout.club.women, 1).map(|b| b[0]);
        if women(leaving.club)? != women(club)? {
            return Err(MoveError::MenAndWomen);
        }
        let (senior, list) = self.senior_team(club, club_uid)?;
        // A team without a list has no block to add to.
        let list = list.ok_or(MoveError::NoRoom { players: 0 })?;
        let elements = list.elements(process)?;
        if elements.contains(&leaving.object) {
            return Err(MoveError::NotListed);
        }
        Ok(Arrival { club_uid, senior, list, elements })
    }

    /// The senior team of the club at `club`, which must point back at it, and its list, if it has one.
    pub(crate) fn senior_team(&self, club: u64, club_uid: u32) -> Result<(u64, Option<PointerList>), MoveError> {
        let (process, layout) = (&self.process, &self.layout);
        let senior = match PointerList::read(process, club + layout.club.teams)? {
            Some(teams) if teams.len() > 0 => process.read_u64(teams.begin)?,
            _ => 0,
        };
        if !is_user_pointer(senior) || process.read_u64(senior + layout.team.club)? != club {
            return Err(MoveError::NotAClub { uid: club_uid });
        }
        Ok((senior, PointerList::read(process, senior + layout.team.players)?))
    }

    /// How many players the list of `club`'s senior team holds, and how many its block has
    /// room for: a move to the club needs one more, unless it releases one of them (see
    /// [`Session::plan_club_move_releasing`]).
    pub fn senior_squad_room(&self, club: &ClubSummary) -> Result<(u64, u64), MoveError> {
        match self.senior_team(club.address, club.uid)?.1 {
            Some(list) => Ok((list.len(), list.room(&self.process, &self.layout.heap)?)),
            None => Ok((0, 0)),
        }
    }

    /// The player `object` goes into the first free slot of `list`, then the list takes it in.
    pub(crate) fn append_patches(
        &self,
        list: &PointerList,
        object: u64,
        patches: &mut Patches,
    ) -> Result<(), MoveError> {
        let slot = self.process.read_bytes(list.end, 8)?;
        patches.push(list.end, slot, object.to_le_bytes());
        let end = list.end + 8;
        let header = [list.end.to_le_bytes(), list.capacity.to_le_bytes()].concat();
        patches.push(list.header + 8, header, [end.to_le_bytes(), list.capacity.max(end).to_le_bytes()].concat());
        Ok(())
    }

    /// The player `leaving` describes plays for `senior` under contract with it from today, on
    /// `terms`, with no squad number, and off the transfer and loan lists.
    fn contract_patches(&self, leaving: &Departure, senior: u64, terms: MoveTerms, patches: &mut Patches) {
        let (p, c) = (&self.layout.player, &self.layout.contract);
        let Departure { object, team, contract, employer, ref record, .. } = *leaving;
        for offset in [c.team, c.senior_team] {
            patches.push(contract + offset, employer.to_le_bytes(), senior.to_le_bytes());
        }
        let date_at = |offset: u64| &record[offset as usize..offset as usize + 4];
        if let Some(today) = self.game_date() {
            for offset in [c.signed, c.joined] {
                // A date the game has not set stays unset.
                if Kind::Date.decode(date_at(offset)) != 0 {
                    patches.push(
                        contract + offset,
                        date_at(offset),
                        Kind::Date.encode(today.to_number(), date_at(offset)),
                    );
                }
            }
        }
        if let Some(wage) = terms.wage {
            let old = &record[c.wage as usize..c.wage as usize + 4];
            patches.push(contract + c.wage, old, Kind::I32.encode(wage, old));
        }
        if let Some(expires) = terms.expires {
            patches.push(contract + c.expires, date_at(c.expires), Kind::Date.encode(expires, date_at(c.expires)));
        }
        let byte = |offset: u64| record[offset as usize];
        patches.push(contract + c.squad_number, [byte(c.squad_number)], [NO_SQUAD_NUMBER]);
        patches.push(contract + c.listing, [byte(c.listing)], [byte(c.listing) & !LISTED]);
        patches.push(object + p.team, team.to_le_bytes(), senior.to_le_bytes());
    }

    /// Writes a planned move. Every write lands only on the bytes the plan found, and is
    /// read back; when one fails, the ones before it are put back. A player leaving a club a
    /// human manager runs leaves its selections first, which are found now.
    pub fn apply_club_move(&self, plan: &MovePlan) -> Result<ClubMove, MoveError> {
        let mut selections = Vec::new();
        for leaver in &plan.leavers {
            selections.extend(self.selection_patches(leaver)?);
        }
        // A list grown for him points at its new buffer first: he is in it already.
        let mut patches = Vec::new();
        for growth in &plan.grow {
            patches.push(self.grown_list(growth)?);
        }
        patches.extend(plan.patches.iter().cloned());
        let all: Vec<Patch> = selections.iter().chain(&patches).cloned().collect();
        self.apply_patches(&all)?;
        let MovePlan { player, from_club, to_club, other, released, .. } = *plan;
        Ok(ClubMove { player, from_club, to_club, other, released, grew: plan.grow.len(), patches, selections })
    }

    /// Moves the player back as he was, provided memory still holds everything the move
    /// wrote. When a write fails, the ones already undone are written again. The selections
    /// he left get him back where nothing has changed them since.
    pub fn undo_club_move(&self, done: &ClubMove) -> Result<(), MoveError> {
        self.undo_patches(&done.patches)?;
        self.restore_selections(&done.selections);
        Ok(())
    }

    /// Writes `patches` in order, each over the bytes it expects, after checking them all;
    /// when one fails, the ones before it are put back. Refused while the checks a change to
    /// the game's lists and records needs have not passed (see [`Session::structure_lock`]).
    pub(crate) fn apply_patches(&self, patches: &[Patch]) -> Result<(), MoveError> {
        if let Some(lock) = self.structure_lock() {
            return Err(MoveError::Locked(lock));
        }
        self.write_patches(patches)
    }

    /// [`Session::apply_patches`] without the checks, for what an undo puts back.
    pub(crate) fn write_patches(&self, patches: &[Patch]) -> Result<(), MoveError> {
        for patch in patches {
            self.expect(patch.address, &patch.before)?;
        }
        for (i, patch) in patches.iter().enumerate() {
            if let Err(err) = self.write_patch(patch.address, &patch.before, &patch.after) {
                for done in patches[..i].iter().rev() {
                    let _ = self.write_patch(done.address, &done.after, &done.before);
                }
                return Err(err);
            }
        }
        Ok(())
    }

    /// Writes back what `patches` found, in reverse order, provided memory still holds all
    /// they wrote; when a write fails, the ones already undone are written again.
    pub(crate) fn undo_patches(&self, patches: &[Patch]) -> Result<(), MoveError> {
        for patch in patches {
            self.expect(patch.address, &patch.after)?;
        }
        for (i, patch) in patches.iter().enumerate().rev() {
            if let Err(err) = self.write_patch(patch.address, &patch.after, &patch.before) {
                for redo in &patches[i + 1..] {
                    let _ = self.write_patch(redo.address, &redo.before, &redo.after);
                }
                return Err(err);
            }
        }
        Ok(())
    }

    /// Checks contract terms against the ranges the editor writes for those fields; an
    /// end date must also come after `today`.
    pub(crate) fn check_terms(&self, terms: MoveTerms, today: Option<FmDate>) -> Result<(), MoveError> {
        let check = |key: &'static str, value: Option<i64>, after: i64| -> Result<(), MoveError> {
            let Some(value) = value else {
                return Ok(());
            };
            let field = self.field(Entity::Player, key).map_err(|_| MoveError::UnusualContract)?;
            let min = field.min.max(after + 1);
            let date = field.kind != Kind::Date || FmDate::from_number(value).is_some();
            if !(min..=field.max).contains(&value) || !date {
                return Err(MoveError::OutOfRange { field: key, value, min, max: field.max });
            }
            Ok(())
        };
        check("contract.wage", terms.wage, i64::MIN)?;
        check("contract.expires", terms.expires, today.map_or(i64::MIN, FmDate::to_number))
    }

    /// Writes `bytes` over `expected`, and reads them back; on a mismatch `expected` goes back.
    pub(crate) fn write_patch(&self, address: u64, expected: &[u8], bytes: &[u8]) -> Result<(), MoveError> {
        self.expect(address, expected)?;
        // The statics of a pool of the game's the editor took a slot of are the plugin's data; the rest is heap.
        let write = |bytes: &[u8]| match self.is_pool_static(address, bytes.len()) {
            true => self.process.write_static(address, bytes),
            false => self.process.write_bytes(address, bytes),
        };
        write(bytes)?;
        if self.process.read_bytes(address, bytes.len())? != bytes {
            let _ = write(expected);
            return Err(MoveError::NotApplied { address });
        }
        Ok(())
    }

    fn expect(&self, address: u64, bytes: &[u8]) -> Result<(), MoveError> {
        if self.process.read_bytes(address, bytes.len())? != bytes {
            return Err(MoveError::Changed { address });
        }
        Ok(())
    }
}

/// The most a transfer fee can be, in pounds: what the game's money fields hold.
pub const MAX_FEE: i64 = 2_000_000_000;

/// An amount of money before and after a fee is paid, in pounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Amounts {
    pub before: i64,
    pub after: i64,
}

/// What a fee does to one club's money.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClubMoney {
    pub club: u32,
    pub balance: Amounts,
    /// The buying club's transfer budget, which the fee comes out of too, down to nothing; or the selling club's,
    /// which takes the share of the fee its board makes available (`finance.transfer_revenue`). None when the board
    /// has set no budget, or for a seller whose share is not known.
    pub transfer_budget: Option<Amounts>,
}

/// What paying a transfer fee does, worked out from the game as it is now: the buying club pays it out of its balance
/// and its transfer budget, and the selling club's balance takes it, its transfer budget the share its board makes
/// available, as the game's own sales do. A club the game keeps no full finances for has none here, and its money
/// stays as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeePlan {
    pub fee: i64,
    pub buyer: Option<ClubMoney>,
    pub seller: Option<ClubMoney>,
}

/// An amount a fee changed, and by how much: undoing the fee takes that back from what the game holds then, as
/// the game will have moved the money on in the meantime.
#[derive(Debug, Clone)]
pub struct Paid {
    target: Target,
    field: Field,
    change: i64,
}

/// What a sale for `fee` does to the selling club's transfer budget `left`: it takes the share `percent` of the fee
/// its board makes available, rounded down, at most `max`. None when the board has set no budget (-1) or the share
/// is not a percentage, and the budget is left alone.
fn sale_budget(left: i64, fee: i64, percent: i64, max: i64) -> Option<Amounts> {
    if left < 0 || !(0..=100).contains(&percent) {
        return None;
    }
    Some(Amounts { before: left, after: (left + fee * percent / 100).min(max) })
}

impl Session {
    /// Works out what paying `fee` does to the money of `buyer` and `seller`. Nothing is written.
    pub fn plan_fee(&self, buyer: &ClubSummary, seller: &ClubSummary, fee: i64) -> Result<FeePlan, EditError> {
        if !(0..=MAX_FEE).contains(&fee) {
            return Err(EditError::OutOfRange { field: "fee".to_owned(), value: fee, min: 0, max: MAX_FEE });
        }
        let balance = self.field(Entity::Club, "finance.balance")?;
        let budget = self.field(Entity::Club, "finance.transfer_budget")?;
        // A club outside the leagues being played has no finance record: its money is left alone.
        let read = |club: &ClubSummary, field: &Field| match self.read_field(&club.target(), field) {
            Ok(value) => Ok(Some(value)),
            Err(EditError::NoRecord { .. }) => Ok(None),
            Err(err) => Err(err),
        };
        let buyer_money = match read(buyer, &balance)? {
            Some(before) => {
                // FM keeps -1 when the board has set no transfer budget.
                let transfer_budget = read(buyer, &budget)?
                    .filter(|&left| left >= 0)
                    .map(|before| Amounts { before, after: (before - fee).max(0) });
                Some(ClubMoney { club: buyer.uid, balance: Amounts { before, after: before - fee }, transfer_budget })
            }
            None => None,
        };
        // The layouts that do not know where the share is kept leave the seller's transfer budget alone.
        let share = self.field(Entity::Club, "finance.transfer_revenue").ok();
        let seller_money = match read(seller, &balance)? {
            Some(before) => {
                let transfer_budget = match &share {
                    Some(share) => match (read(seller, share)?, read(seller, &budget)?) {
                        (Some(percent), Some(left)) => sale_budget(left, fee, percent, budget.max),
                        _ => None,
                    },
                    None => None,
                };
                Some(ClubMoney { club: seller.uid, balance: Amounts { before, after: before + fee }, transfer_budget })
            }
            None => None,
        };
        for money in [buyer_money, seller_money].into_iter().flatten() {
            if !balance.takes(money.balance.after) {
                let (min, max) = (balance.min, balance.max);
                return Err(EditError::OutOfRange { field: balance.key.clone(), value: money.balance.after, min, max });
            }
        }
        Ok(FeePlan { fee, buyer: buyer_money, seller: seller_money })
    }

    /// Pays `plan`, each amount only while the game still holds what it was worked out from. When one cannot be
    /// written, those already written are taken back.
    pub fn pay_fee(&self, plan: &FeePlan, buyer: &ClubSummary, seller: &ClubSummary) -> Result<Vec<Paid>, EditError> {
        let balance = self.field(Entity::Club, "finance.balance")?;
        let budget = self.field(Entity::Club, "finance.transfer_budget")?;
        let mut steps: Vec<(Target, &Field, Amounts)> = Vec::new();
        if let Some(money) = plan.buyer {
            steps.push((buyer.target(), &balance, money.balance));
            if let Some(left) = money.transfer_budget {
                steps.push((buyer.target(), &budget, left));
            }
        }
        if let Some(money) = plan.seller {
            steps.push((seller.target(), &balance, money.balance));
            if let Some(gained) = money.transfer_budget {
                steps.push((seller.target(), &budget, gained));
            }
        }
        let mut paid = Vec::new();
        for (target, field, amounts) in steps {
            if amounts.before == amounts.after {
                continue;
            }
            if let Err(err) = self.write_field_if(&target, field, amounts.before, amounts.after) {
                let _ = self.refund_fee(&paid);
                return Err(err);
            }
            paid.push(Paid { target, field: field.clone(), change: amounts.after - amounts.before });
        }
        Ok(paid)
    }

    /// Takes back what a fee changed, each amount by its change from what the game holds now, kept within what the
    /// field holds. When one cannot be taken back, those already taken back are paid again.
    pub fn refund_fee(&self, paid: &[Paid]) -> Result<(), EditError> {
        for (i, step) in paid.iter().enumerate().rev() {
            if let Err(err) = self.shift_by(step, -step.change) {
                for done in &paid[i + 1..] {
                    let _ = self.shift_by(done, done.change);
                }
                return Err(err);
            }
        }
        Ok(())
    }

    /// Pays a fee taken back again, after an undo that could not finish.
    pub fn repay_fee(&self, paid: &[Paid]) -> Result<(), EditError> {
        for step in paid {
            self.shift_by(step, step.change)?;
        }
        Ok(())
    }

    /// Adds `by` to the amount `step` changed, as the game holds it now, kept within what its field holds.
    fn shift_by(&self, step: &Paid, by: i64) -> Result<(), EditError> {
        let now = self.read_field(&step.target, &step.field)?;
        let next = (now + by).clamp(step.field.min, step.field.max);
        self.write_field_if(&step.target, &step.field, now, next).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_that_change_nothing_are_left_out() {
        let mut patches = Patches::default();
        patches.push(0x10, [1u8], [1u8]);
        patches.push(0x20, [1u8], [2u8]);
        assert_eq!(patches.0, [Patch { address: 0x20, before: vec![1], after: vec![2] }]);
    }

    #[test]
    fn the_weakest_fit_player_is_suggested_first() {
        let maker = |uid, injured, captain| RoomMaker { uid, injured, captain };
        let found = vec![
            (maker(1, true, false), 60),
            (maker(2, false, true), 55),
            (maker(3, false, false), 90),
            (maker(4, false, false), 70),
            // As weak as the one before him: after him, as the list has them.
            (maker(5, false, false), 70),
            (maker(6, true, true), 50),
        ];
        let order: Vec<u32> = suggestion_order(found).iter().map(|maker| maker.uid).collect();
        assert_eq!(order, [4, 5, 3, 6, 2, 1]);
    }

    #[test]
    fn a_sale_adds_the_boards_share_of_the_fee_to_the_transfer_budget() {
        // What `finance.transfer_budget` takes.
        let max = 2_000_000_000;
        // Barcelona's sale of Antonio Nusa: all of £101.7M, at 100%.
        assert_eq!(
            sale_budget(384_681_863, 101_700_000, 100, max),
            Some(Amounts { before: 384_681_863, after: 486_381_863 })
        );
        assert_eq!(sale_budget(1_000, 999, 25, max), Some(Amounts { before: 1_000, after: 1_249 }));
        assert_eq!(sale_budget(1_000, 5_000, 0, max), Some(Amounts { before: 1_000, after: 1_000 }));
        assert_eq!(sale_budget(max - 10, 1_000, 100, max).map(|a| a.after), Some(max));
        // No budget set, or a share that is not a percentage: left alone.
        assert_eq!(sale_budget(-1, 1_000, 100, max), None);
        assert_eq!(sale_budget(1_000, 1_000, 101, max), None);
        assert_eq!(sale_budget(1_000, 1_000, -1, max), None);
    }
}
