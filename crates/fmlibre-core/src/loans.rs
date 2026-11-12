//! A player's loan: the second contract a player on loan has, with the team he is on loan to.
//!
//! The game keeps it apart from his contract with his own club (see [`LoanLayout`]). The editor reads where he is on
//! loan, from when to when and on what terms, and changes the day the loan ends (the field `loan.expires`, see
//! [`crate::fields`]). It
//! never ends a loan itself. The game takes that day as the loan's last one: when it is over, the game ends the loan
//! and the player is back at his club the next day, as at the end of any loan. So a loan brought forward to end on the
//! game's date brings him back one day on (on the Game Pass save, Onana and Bayındır, made to end the next day, were
//! back two days on).
//!
//! [`LoanLayout`]: crate::profile::LoanLayout

use fmlibre_mem::is_user_pointer;
use serde::{Deserialize, Serialize};

use crate::clubs::ClubSummary;
use crate::date::FmDate;
use crate::edit::Kind;
use crate::players::PlayerSummary;
use crate::profile::{ContractLayout, LoanLayout, LoanStartLayout, LoanTermsLayout};
use crate::rtti;
use crate::session::Session;
use crate::signing::NewContract;
use crate::text::read_fm_string;
use crate::transfer::{LISTED, MoveError, MovePlan, MoveTerms, NO_SQUAD_NUMBER, PLAYER_JOB, Patches};

/// The bits of a loan's flags its terms are made of (see [`LoanTerms`]): against his own club, cup matches, recall,
/// and that it cannot be terminated.
const AGAINST_OWN_CLUB: u16 = 0x1;
const CUP_MATCHES: u16 = 0x2;
const RECALL: u16 = 0x4;
const NOT_TERMINABLE: u16 = 0x1000;

/// A loan for the editor to start: until when, the wage the club he goes to pays, and what the clubs agree. No option
/// to buy and no monthly fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLoan {
    /// The loan's last day, as a number like 20330630. By default the end of the season under way (30 June), or the
    /// end of his contract when that comes first.
    pub until: Option<i64>,
    /// Pounds per week; his own contract's by default.
    pub wage: Option<i64>,
    pub can_be_recalled: bool,
    pub can_be_terminated: bool,
    pub cup_matches: bool,
    pub against_own_club: bool,
}

impl Default for NewLoan {
    fn default() -> Self {
        Self {
            until: None,
            wage: None,
            can_be_recalled: true,
            can_be_terminated: true,
            cup_matches: true,
            against_own_club: false,
        }
    }
}

impl NewLoan {
    /// The bits of the flags these terms set (the other bits are the model's).
    fn flags(self) -> u16 {
        let bit = |on: bool, bit: u16| if on { bit } else { 0 };
        bit(self.against_own_club, AGAINST_OWN_CLUB)
            | bit(self.cup_matches, CUP_MATCHES)
            | bit(self.can_be_recalled, RECALL)
            | bit(!self.can_be_terminated, NOT_TERMINABLE)
    }
}

/// What a loan contract the editor makes holds of its own (see [`Session::plan_loan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoanContract {
    /// The player's identity block, which contracts point at.
    owner: u64,
    /// The senior team he goes to.
    team: u64,
    /// The first and the last day of the loan, as numbers like 20330529.
    from: i64,
    until: i64,
    /// Pounds per week.
    wage: i64,
    /// The bits of the flags its terms set (see [`NewLoan::flags`]).
    flags: u16,
}

impl LoanContract {
    /// The bytes of the loan contract: `model`'s (a running loan's), with this loan's own, the squad status and how
    /// happy he is with his contract from `own` (his own contract's bytes), no squad number, off the lists, no option
    /// to buy and no fees, and the model's own records and counts of what he played cleared.
    fn bytes(
        self,
        model: &[u8],
        own: &[u8],
        c: &ContractLayout,
        start: &LoanStartLayout,
        terms: &LoanTermsLayout,
    ) -> Vec<u8> {
        let mut bytes = model.to_vec();
        let mut put = |offset: u64, value: &[u8]| bytes[at(offset, value.len())].copy_from_slice(value);
        put(c.owner, &self.owner.to_le_bytes());
        put(c.team, &self.team.to_le_bytes());
        put(c.senior_team, &self.team.to_le_bytes());
        put(c.job, &[PLAYER_JOB]);
        // The dates keep the flags the model's have.
        for offset in [c.signed, c.joined] {
            put(offset, &Kind::Date.encode(self.from, &model[at(offset, 4)]));
        }
        put(c.expires, &Kind::Date.encode(self.until, &model[at(c.expires, 4)]));
        put(c.wage, &Kind::I32.encode(self.wage, &model[at(c.wage, 4)]));
        put(start.wage_again, &Kind::I32.encode(self.wage, &model[at(start.wage_again, 4)]));
        put(c.squad_status, &[own[c.squad_status as usize]]);
        put(c.happiness, &[own[c.happiness as usize]]);
        put(c.playing_time_happiness, &[own[c.playing_time_happiness as usize]]);
        put(c.squad_number, &[NO_SQUAD_NUMBER]);
        put(c.listing, &[model[c.listing as usize] & !LISTED]);
        for &(offset, len) in &start.cleared {
            put(offset, &vec![0; len as usize]);
        }
        let terms_bits = AGAINST_OWN_CLUB | CUP_MATCHES | RECALL | NOT_TERMINABLE;
        let u16_of = |offset: u64| u16::from_le_bytes([model[offset as usize], model[offset as usize + 1]]);
        // The new loan's flags take the shape the model's have (see [`LoanFlags`]).
        match LoanFlags::of(u16_of(terms.flags), u16_of(start.agreed_flags), start.in_force) {
            Some(LoanFlags::Agreed(model_flags)) => {
                let agreed = (model_flags & !terms_bits) | self.flags;
                put(start.agreed_flags, &agreed.to_le_bytes());
                put(terms.flags, &(agreed | start.in_force).to_le_bytes());
            }
            running => {
                let model_flags = match running {
                    Some(LoanFlags::Running(flags)) => flags,
                    _ => u16_of(terms.flags),
                };
                let flags = (model_flags & !terms_bits & !start.in_force) | self.flags;
                put(start.agreed_flags, &0u16.to_le_bytes());
                put(terms.flags, &flags.to_le_bytes());
            }
        }
        put(terms.option_fee, &(-1i32).to_le_bytes());
        put(terms.monthly_fee, &0i32.to_le_bytes());
        put(terms.unused_fee, &0i32.to_le_bytes());
        bytes
    }
}

/// The two shapes the flags of a running loan take. In the close season (a save of 29 May 2033 on the Mac, one of 14
/// June 2033 on Windows) most loans hold the flags as agreed at `agreed_flags` and the terms' flags are those with
/// `in_force` set; by January 2034 (Windows, 4 January) the game has cleared both in all 1,046 loans, those it began
/// that month among them: `agreed_flags` reads 0 and the terms' flags are the terms alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoanFlags {
    /// The flags at `agreed_flags`.
    Agreed(u16),
    /// The terms' flags.
    Running(u16),
}

impl LoanFlags {
    /// Which shape the terms' flags `flags` and those at `agreed_flags`, `agreed`, are in, if either.
    fn of(flags: u16, agreed: u16, in_force: u16) -> Option<Self> {
        if agreed != 0 && flags == agreed | in_force {
            Some(Self::Agreed(agreed))
        } else if agreed == 0 && flags & in_force == 0 {
            Some(Self::Running(flags))
        } else {
            None
        }
    }
}

/// The bytes of a record from `offset` on, `len` of them.
fn at(offset: u64, len: usize) -> std::ops::Range<usize> {
    offset as usize..offset as usize + len
}

/// The last day of the season under way on `today`, as a number like 20330630: seasons end on 30 June.
fn season_end(today: FmDate) -> i64 {
    let year = if (today.month, today.day) <= (6, 30) { today.year } else { today.year + 1 };
    i64::from(year) * 10_000 + 630
}

/// Where a player is on loan, as his loan contract says, and the club he belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoanDetails {
    /// The club he is on loan to.
    pub club_uid: Option<u32>,
    pub club: Option<String>,
    /// The club his own contract is with.
    pub parent_uid: Option<u32>,
    pub parent: Option<String>,
    /// When the loan started, and the day it ends.
    pub from: Option<FmDate>,
    pub until: Option<FmDate>,
    /// His squad number at the club he is on loan to.
    pub squad_number: Option<u8>,
    /// Pounds per week, as the loan contract holds it.
    pub wage: i32,
    /// Where its end stands against the game's date.
    pub ends: LoanEnd,
    /// Whether the editor brought it forward this session (see the app's change log); the core never sets it.
    pub recalled: bool,
    /// What the clubs agreed, as the game's Loan Club tab shows it; none where the layout does not place it, or what
    /// is there does not read as terms.
    pub terms: Option<LoanTerms>,
}

/// The terms of a loan, as the game's Loan Club tab shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoanTerms {
    /// His own club may recall him ("Can be recalled"). The editor's recall does not go through the game's: it moves
    /// the loan's last day, which the game holds to either way.
    pub can_be_recalled: bool,
    /// The loan may be ended early: false where the game says "Loan cannot be terminated".
    pub can_be_terminated: bool,
    /// He may play in cup matches.
    pub cup_matches: bool,
    /// He may play against his own club.
    pub against_own_club: bool,
    /// Pounds: the fee for which the club he is on loan to may sign him for good ("Optional Future Fee").
    pub option_fee: Option<i32>,
    /// Pounds a month that club pays his own while he plays ("Monthly playing fee").
    pub monthly_fee: Option<i32>,
    /// Pounds a month it pays while he is not used ("Unused Monthly Fee").
    pub unused_fee: Option<i32>,
    /// The flags as the game keeps them, the bits the game shows nothing for too.
    pub flags: u16,
}

impl LoanTerms {
    /// The terms from what a loan contract holds: its flags and its three amounts (-1 for none). `None` when an
    /// amount is below -1, which no loan holds: then the bytes are not terms.
    pub fn decode(flags: u16, option_fee: i32, monthly_fee: i32, unused_fee: i32) -> Option<Self> {
        if [option_fee, monthly_fee, unused_fee].iter().any(|&amount| amount < -1) {
            return None;
        }
        let amount = |value: i32| (value >= 0).then_some(value);
        Some(Self {
            can_be_recalled: flags & 0x4 != 0,
            can_be_terminated: flags & 0x1000 == 0,
            cup_matches: flags & 0x2 != 0,
            against_own_club: flags & 0x1 != 0,
            option_fee: amount(option_fee),
            monthly_fee: amount(monthly_fee),
            unused_fee: amount(unused_fee),
            flags,
        })
    }
}

/// Where a loan's end stands against the game's date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoanEnd {
    /// After the game's date.
    Later,
    /// On the game's date: its last day. When the day is over the game ends it, and he is back at his club the next day.
    Today,
    /// Before the game's date, and he is still on loan: the game has not ended it when its day was over.
    Passed,
    /// The game's date, or the loan's end, is not known.
    Unknown,
}

impl LoanEnd {
    /// Where the end `until` stands against the game's date `today`.
    pub fn of(until: Option<FmDate>, today: Option<FmDate>) -> Self {
        match (until, today) {
            (Some(until), Some(today)) if until > today => LoanEnd::Later,
            (Some(until), Some(today)) if until == today => LoanEnd::Today,
            (Some(_), Some(_)) => LoanEnd::Passed,
            _ => LoanEnd::Unknown,
        }
    }
}

impl Session {
    /// Whether the editor can find loan contracts in this game: the layout places them and the game names their class.
    pub fn loans_known(&self) -> bool {
        !self.loan_vtables().is_empty()
    }

    /// The vtables of loan contracts: none when the layout does not place loans or the game does not name the class.
    fn loan_vtables(&self) -> &[u64] {
        self.loan_class.get_or_init(|| match &self.layout.loan {
            Some(loan) => rtti::vtables(&self.process, &self.plugin, &loan.class),
            None => Vec::new(),
        })
    }

    /// The loan contract of the player whose object is at `object`, with his person block shifted by `shift`: `None`
    /// when he is not on loan, or when what his record leads to is not a loan contract that points back at him.
    pub fn loan_contract(&self, object: u64, shift: u64) -> fmlibre_mem::Result<Option<u64>> {
        let Some(loan) = &self.layout.loan else {
            return Ok(None);
        };
        let p = &self.layout.player;
        let record = self.process.read_u64(object + p.at(loan.record, shift))?;
        if !is_user_pointer(record) {
            return Ok(None);
        }
        let contract = self.process.read_u64(record + loan.contract)?;
        if !is_user_pointer(contract) || !self.loan_vtables().contains(&self.process.read_u64(contract)?) {
            return Ok(None);
        }
        let owner = self.process.read_u64(contract + self.layout.contract.owner)?;
        Ok((owner == object + p.identity() + shift).then_some(contract))
    }

    /// Where `player` is on loan, or `None` when he is not (or the editor cannot tell in this game).
    pub fn loan(&self, player: &PlayerSummary) -> fmlibre_mem::Result<Option<LoanDetails>> {
        let p = &self.layout.player;
        if self.process.read_u32(player.address + p.at(p.uid, player.shift))? != player.uid {
            return Ok(None);
        }
        let Some(contract) = self.loan_contract(player.address, player.shift)? else {
            return Ok(None);
        };
        let c = &self.layout.contract;
        let terms_layout = self.layout.loan.as_ref().and_then(|loan| loan.terms.as_ref());
        let length = terms_layout.map_or(0, |t| t.end()).max(c.squad_number + 1);
        let bytes = self.process.read_bytes(contract, length as usize)?;
        let u16_at = |at: u64| u16::from_le_bytes([bytes[at as usize], bytes[at as usize + 1]]);
        let i32_at = |at: u64| i32::from_le_bytes(bytes[at as usize..at as usize + 4].try_into().expect("4 bytes"));
        let date = |at: u64| FmDate::from_raw(u16_at(at), u16_at(at + 2));
        let team = u64::from_le_bytes(bytes[c.team as usize..c.team as usize + 8].try_into().expect("8 bytes"));
        let own = self.process.read_u64(player.address + p.at(p.contract, player.shift))?;
        let parent_team = if is_user_pointer(own) { self.process.read_u64(own + c.team)? } else { 0 };
        let (club_uid, club) = self.club_of_team(team);
        let (parent_uid, parent) = self.club_of_team(parent_team);
        let number = bytes[c.squad_number as usize];
        let until = date(c.expires);
        Ok(Some(LoanDetails {
            club_uid,
            club,
            parent_uid,
            parent,
            from: date(c.signed),
            until,
            squad_number: (number != 255).then_some(number),
            wage: i32_at(c.wage),
            ends: LoanEnd::of(until, self.game_date()),
            recalled: false,
            terms: terms_layout.and_then(|t| {
                LoanTerms::decode(u16_at(t.flags), i32_at(t.option_fee), i32_at(t.monthly_fee), i32_at(t.unused_fee))
            }),
        }))
    }

    /// Works out sending `player` on loan to the senior team of `club` on `loan`. `human_clubs` are the UIDs of the
    /// clubs human managers run: a player who plays for one leaves its selections. Nothing is written.
    ///
    /// A loan is what the game makes one of, as read across the loans of a save (docs/recon/fm27-loans.md):
    /// 1. a loan contract, in the first free slot of the game's pool of them: a copy of a running loan's without an
    ///    option to buy, now his, with the club's senior team, from today until the loan's last day, on the wage given
    ///    or his own, with his squad status and how happy he is with his contract, no squad number, off the lists, the
    ///    terms given, no fees, and none of the model's own records or counts of what he played;
    /// 2. the record of his other contracts pointing at it: his own record, whose place for a loan is empty, or a new
    ///    one in the first free slot of the pool of records;
    /// 3. the club's senior team's list holding him, grown when it is full;
    /// 4. his team: the club's senior team. His own contract, and the lists of his club, are left as they are; a
    ///    captaincy of the club he played for is left empty.
    ///
    /// The game then holds him as on loan, and ends the loan as it ends its own (see [`Session::loan`]).
    pub fn plan_loan(
        &self,
        player: &PlayerSummary,
        club: &ClubSummary,
        loan: NewLoan,
        human_clubs: &[u32],
    ) -> Result<MovePlan, MoveError> {
        let (process, layout) = (&self.process, &self.layout);
        let (p, c, t) = (&layout.player, &layout.contract, &layout.team);
        let loans = layout.loan.as_ref().ok_or(MoveError::NoLoanStart)?;
        let (Some(start), Some(terms), Some(pools)) = (&loans.start, &loans.terms, &layout.pool) else {
            return Err(MoveError::NoLoanStart);
        };
        let today = self.game_date().ok_or(MoveError::NoDate)?;
        let object = player.address;
        if process.read_u32(object + p.at(p.uid, player.shift))? != player.uid {
            return Err(MoveError::Moved { uid: player.uid });
        }
        if player.shift != 0 {
            return Err(MoveError::PlayerCoach);
        }
        if self.loan_contract(object, 0)?.is_some() {
            return Err(MoveError::OnLoan);
        }
        let team = process.read_u64(object + p.team)?;
        let own = process.read_u64(object + p.contract)?;
        if !is_user_pointer(team) || !is_user_pointer(own) {
            return Err(MoveError::FreeAgent);
        }

        // His own contract must be his, a player's, with one team to its name: the club he is loaned from.
        let contract = process.read_bytes(own, c.end() as usize)?;
        let word = |offset: u64| u64::from_le_bytes(contract[at(offset, 8)].try_into().expect("8 bytes"));
        let employer = word(c.team);
        if word(c.owner) != object + p.identity()
            || contract[c.job as usize] != PLAYER_JOB
            || word(c.senior_team) != employer
            || !is_user_pointer(employer)
        {
            return Err(MoveError::UnusualContract);
        }
        let parent = process.read_u64(employer + t.club)?;
        let playing = process.read_u64(team + t.club)?;
        if !is_user_pointer(parent) || !is_user_pointer(playing) {
            return Err(MoveError::UnusualContract);
        }
        let parent_uid = process.read_u32(parent + layout.club.uid)?;

        // The club he goes to: another than his own and the one he plays for, of the same kind.
        if process.read_u32(club.address + layout.club.uid)? != club.uid {
            return Err(MoveError::NotAClub { uid: club.uid });
        }
        if club.address == parent || club.address == playing {
            return Err(MoveError::SameClub);
        }
        let women = process.read_bytes(club.address + layout.club.women, 1)?[0] != 0;
        if player.woman != Some(women) {
            return Err(MoveError::MenAndWomen);
        }

        // Until a day after today, and not past his contract.
        let own_end = Kind::Date.decode(&contract[at(c.expires, 4)]);
        let latest = if own_end > 0 { own_end } else { i64::MAX };
        let until = loan.until.unwrap_or_else(|| season_end(today).min(latest));
        if until <= today.to_number() || until > latest || FmDate::from_number(until).is_none() {
            return Err(MoveError::OutOfRange {
                field: "loan.expires",
                value: until,
                min: today.to_number() + 1,
                max: own_end,
            });
        }
        self.check_terms(MoveTerms { wage: loan.wage, expires: None }, None)?;
        let wage = loan.wage.unwrap_or_else(|| Kind::I32.decode(&contract[at(c.wage, 4)]));

        let (senior, list) = self.senior_team(club.address, club.uid)?;
        let list = list.ok_or(MoveError::NoRoom { players: 0 })?;
        let elements = list.elements(process)?;
        if elements.contains(&object) {
            return Err(MoveError::NotListed);
        }

        // The model, and the slot of the new loan contract.
        let (model, model_record) = self.model_loan(loans, start, terms)?;
        let pool = self.pool_holding(model, start.size)?;
        let taken = pool.take(process, pools)?;
        let model = process.read_bytes(model, start.size as usize)?;
        let made = LoanContract {
            owner: object + p.identity(),
            team: senior,
            from: today.to_number(),
            until,
            wage,
            flags: loan.flags(),
        };
        let bytes = made.bytes(&model, &contract, c, start, terms);
        let squad_status = contract[c.squad_status as usize];

        let mut patches = Patches::default();
        patches.push(taken.pool.address, taken.pool.before, taken.pool.after);
        patches.push(taken.slot, taken.was, bytes);
        // The record of his other contracts: his own, with nothing in the place of a loan (a trial contract is left
        // alone), or a new one.
        let record_at = object + p.at(loans.record, 0);
        let record = process.read_u64(record_at)?;
        if is_user_pointer(record) {
            let held = process.read_u64(record + loans.contract)?;
            if held != 0 {
                return Err(MoveError::UnusualContract);
            }
            patches.push(record + loans.contract, 0u64.to_le_bytes(), taken.slot.to_le_bytes());
        } else if record == 0 {
            let records = self.pool_holding(model_record, start.record_size)?;
            let new = records.take(process, pools)?;
            let mut held = vec![0; start.record_size as usize];
            held[at(loans.contract, 8)].copy_from_slice(&taken.slot.to_le_bytes());
            patches.push(new.pool.address, new.pool.before, new.pool.after);
            patches.push(new.slot, new.was, held);
            patches.push(record_at, 0u64.to_le_bytes(), new.slot.to_le_bytes());
        } else {
            return Err(MoveError::UnusualContract);
        }
        let grow = self.join_squad(&list, &elements, object, &mut patches)?;

        // He no longer plays for the club he played for: a captaincy there is left empty, and its selections lose him
        // when a human manager runs it.
        let playing_uid = process.read_u32(playing + layout.club.uid)?;
        let teams = match crate::transfer::PointerList::read(process, playing + layout.club.teams)? {
            Some(teams) => teams.elements(process)?,
            None => Vec::new(),
        };
        let block = object + p.identity();
        for &each in &teams {
            for offset in [t.captain, t.vice_captain] {
                if process.read_u64(each + offset)? == block {
                    patches.push(each + offset, block.to_le_bytes(), 0u64.to_le_bytes());
                }
            }
        }
        let leavers = if human_clubs.contains(&playing_uid) {
            if !self.selections_known() {
                return Err(MoveError::HumanClub);
            }
            vec![self.leaver(object, &teams)?]
        } else {
            Vec::new()
        };
        patches.push(object + p.team, team.to_le_bytes(), senior.to_le_bytes());
        Ok(MovePlan {
            player: player.uid,
            from_club: parent_uid,
            to_club: club.uid,
            other: None,
            released: None,
            patches: patches.0,
            leavers,
            grow: grow.into_iter().collect(),
            // The loan contract the editor makes, for the window to show: its wage, its last day, and his status.
            contract: Some(NewContract { wage, expires: until, squad_status }),
        })
    }

    /// A loan contract to model a new one on, and the record of other contracts of the player it is his loan of: the
    /// first in the game's pool of them of a running loan (his record leads to it) with no option to buy, whose flags
    /// are as most loans' are (see [`LoanStartLayout`]), and with none of the model's own records in the places a new
    /// one clears.
    fn model_loan(
        &self,
        loans: &LoanLayout,
        start: &LoanStartLayout,
        terms: &LoanTermsLayout,
    ) -> Result<(u64, u64), MoveError> {
        let (process, p) = (&self.process, &self.layout.player);
        let vtables = self.loan_vtables();
        let size = start.size;
        let holds_loans = |pool: &crate::ObjectPool| {
            u64::from(pool.slot_size) == size
                && (0..pool.slots_of(0).min(16))
                    .any(|slot| process.read_u64(pool.chunks[0] + slot * size).is_ok_and(|w| vtables.contains(&w)))
        };
        let pool = self.object_pools().into_iter().find(holds_loans).ok_or(MoveError::NoLoanModel)?;
        for (index, &chunk) in pool.chunks.iter().enumerate() {
            let slots = pool.slots_of(index);
            let Ok(bytes) = process.read_bytes(chunk, (slots * size) as usize) else {
                continue;
            };
            for (n, slot) in bytes.chunks_exact(size as usize).enumerate() {
                let address = chunk + n as u64 * size;
                let word = |offset: u64| u64::from_le_bytes(slot[at(offset, 8)].try_into().expect("8 bytes"));
                let u16_at = |offset: u64| u16::from_le_bytes([slot[offset as usize], slot[offset as usize + 1]]);
                let i32_at = |offset: u64| i32::from_le_bytes(slot[at(offset, 4)].try_into().expect("4 bytes"));
                let typical = vtables.contains(&word(0))
                    && LoanFlags::of(u16_at(terms.flags), u16_at(start.agreed_flags), start.in_force).is_some()
                    && i32_at(terms.option_fee) == -1
                    && start.cleared.iter().filter(|&&(_, len)| len == 8).all(|&(offset, _)| word(offset) == 0);
                if !typical {
                    continue;
                }
                // A running loan: its player's record leads to it (an offer's contract is held by no record).
                let object = word(self.layout.contract.owner).wrapping_sub(p.identity());
                if self.loan_contract(object, 0).ok().flatten() == Some(address) {
                    let record = process.read_u64(object + p.at(loans.record, 0))?;
                    return Ok((address, record));
                }
            }
        }
        Err(MoveError::NoLoanModel)
    }

    /// The UID and name of the club of the team at `team`, as far as they can be read.
    fn club_of_team(&self, team: u64) -> (Option<u32>, Option<String>) {
        if !is_user_pointer(team) {
            return (None, None);
        }
        let Some(club) = self.process.read_u64(team + self.layout.team.club).ok().filter(|&c| is_user_pointer(c))
        else {
            return (None, None);
        };
        let uid = self.process.read_u32(club + self.layout.club.uid).ok();
        let name =
            self.process.read_u64(club + self.layout.club.name).ok().and_then(|at| read_fm_string(&self.process, at));
        (uid, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loan_ends_later_today_or_has_passed() {
        let day = |day: u8| Some(FmDate { year: 2026, month: 9, day });
        assert_eq!(LoanEnd::of(day(18), day(17)), LoanEnd::Later);
        assert_eq!(LoanEnd::of(day(17), day(17)), LoanEnd::Today);
        assert_eq!(LoanEnd::of(day(16), day(17)), LoanEnd::Passed);
        assert_eq!(LoanEnd::of(None, day(17)), LoanEnd::Unknown);
        assert_eq!(LoanEnd::of(day(17), None), LoanEnd::Unknown);
    }

    #[test]
    fn a_new_loan_sets_the_bits_of_its_terms() {
        let usual = NewLoan::default();
        assert_eq!(usual.flags(), CUP_MATCHES | RECALL);
        let strict = NewLoan {
            can_be_recalled: false,
            can_be_terminated: false,
            cup_matches: false,
            against_own_club: true,
            ..usual
        };
        assert_eq!(strict.flags(), AGAINST_OWN_CLUB | NOT_TERMINABLE);
        // As the game reads them back.
        let read = LoanTerms::decode(usual.flags(), -1, 0, 0).expect("terms");
        assert!(read.can_be_recalled && read.can_be_terminated && read.cup_matches && !read.against_own_club);
    }

    #[test]
    fn a_new_loan_runs_to_the_end_of_the_season() {
        let on = |number| season_end(FmDate::from_number(number).expect("a date"));
        assert_eq!(on(20330529), 20330630);
        assert_eq!(on(20330630), 20330630);
        assert_eq!(on(20330701), 20340630);
        assert_eq!(on(20331231), 20340630);
    }

    /// The Mac's layout, the only one that says how to start a loan so far.
    fn mac_layout() -> crate::profile::Layout {
        toml::from_str(include_str!("../../../profiles/fm27/layout-macos.toml")).expect("the Mac layout")
    }

    #[test]
    fn the_mac_layout_says_how_to_start_a_loan() {
        let layout = mac_layout();
        let loan = layout.loan.as_ref().expect("loans");
        let (start, terms) = (loan.start.as_ref().expect("loan.start"), loan.terms.as_ref().expect("loan.terms"));
        assert_eq!((start.size, start.record_size), (0x100, 0x20));
        assert!(terms.end() <= start.size && start.wage_again + 4 <= start.size);
        assert!(start.cleared.iter().all(|&(offset, len)| offset + len <= start.size));
        // The pointers a loan has of its own: the option to buy's (+0x68) among them.
        assert!(start.cleared.contains(&(0x68, 8)));
        // Nothing cleared is a field a new loan writes.
        let c = &layout.contract;
        let written = [
            (c.owner, 8),
            (c.team, 8),
            (c.senior_team, 8),
            (c.wage, 4),
            (c.signed, 4),
            (c.expires, 4),
            (c.joined, 4),
            (c.squad_status, 1),
            (c.squad_number, 1),
            (start.agreed_flags, 2),
            (terms.flags, 2),
            (start.wage_again, 4),
        ];
        for &(offset, len) in &start.cleared {
            assert!(written.iter().all(|&(at, n)| offset + len <= at || at + n <= offset), "{offset:#x} is written");
        }
        assert_eq!(start.in_force, 0x2000);
    }

    #[test]
    fn a_new_loan_contract_is_the_model_with_this_loan_in_it() {
        let layout = mac_layout();
        let c = &layout.contract;
        let loan = layout.loan.as_ref().expect("loans");
        let (start, terms) = (loan.start.as_ref().expect("loan.start"), loan.terms.as_ref().expect("loan.terms"));
        // A model with something in every byte, flags the terms do not set (bit 15), and on both lists.
        let mut model = vec![0xAA; start.size as usize];
        model[at(start.agreed_flags, 2)].copy_from_slice(&0x8107u16.to_le_bytes());
        model[at(terms.flags, 2)].copy_from_slice(&(0x8107u16 | start.in_force).to_le_bytes());
        model[c.listing as usize] = 0x80 | LISTED;
        // His own contract: Youngster (13), how happy he is with it, and his playing time.
        let mut own = vec![0; c.end() as usize];
        own[c.squad_status as usize] = 13;
        own[c.happiness as usize] = 7;
        own[c.playing_time_happiness as usize] = 9;
        let made = LoanContract {
            owner: 0x4_3C9D_0A48,
            team: 0xA_0595_4D80,
            from: 20330529,
            until: 20330630,
            wage: 90,
            flags: NewLoan::default().flags(),
        };
        let bytes = made.bytes(&model, &own, c, start, terms);
        assert_eq!(bytes.len(), model.len());

        let word = |offset: u64| u64::from_le_bytes(bytes[at(offset, 8)].try_into().expect("8 bytes"));
        let u16_at = |offset: u64| u16::from_le_bytes(bytes[at(offset, 2)].try_into().expect("2 bytes"));
        let i32_at = |offset: u64| i32::from_le_bytes(bytes[at(offset, 4)].try_into().expect("4 bytes"));
        assert_eq!(word(c.owner), made.owner);
        assert_eq!((word(c.team), word(c.senior_team)), (made.team, made.team));
        assert_eq!(bytes[c.job as usize], PLAYER_JOB);
        for (offset, day) in [(c.signed, made.from), (c.joined, made.from), (c.expires, made.until)] {
            assert_eq!(Kind::Date.decode(&bytes[at(offset, 4)]), day);
        }
        assert_eq!(Kind::I32.decode(&bytes[at(c.wage, 4)]), 90);
        assert_eq!(i32_at(start.wage_again), 90);
        assert_eq!(
            (bytes[c.squad_status as usize], bytes[c.happiness as usize], bytes[c.playing_time_happiness as usize]),
            (13, 7, 9)
        );
        assert_eq!(bytes[c.squad_number as usize], NO_SQUAD_NUMBER);
        assert_eq!(bytes[c.listing as usize], 0x80);
        for &(offset, len) in &start.cleared {
            assert!(bytes[at(offset, len as usize)].iter().all(|&b| b == 0), "{offset:#x} cleared");
        }
        // The terms given, the model's bits that are no terms (15 and 8), and while it runs the bit the game sets.
        assert_eq!(u16_at(start.agreed_flags), 0x8100 | CUP_MATCHES | RECALL);
        assert_eq!(u16_at(terms.flags), 0x8100 | CUP_MATCHES | RECALL | start.in_force);
        assert_eq!((i32_at(terms.option_fee), i32_at(terms.monthly_fee), i32_at(terms.unused_fee)), (-1, 0, 0));
        // The rest is the model's: the constant at +0xA8 every loan holds, for one.
        assert_eq!(bytes[0xA8], 0xAA);
    }

    #[test]
    fn a_new_loan_in_the_season_has_its_flags_as_the_game_has_them_then() {
        let layout = mac_layout();
        let c = &layout.contract;
        let loan = layout.loan.as_ref().expect("loans");
        let (start, terms) = (loan.start.as_ref().expect("loan.start"), loan.terms.as_ref().expect("loan.terms"));
        // Andy King's, Manchester City to Girona from 3 January 2034: the flags as agreed cleared, the terms' 0x000E.
        let mut model = vec![0xAA; start.size as usize];
        model[at(start.agreed_flags, 2)].copy_from_slice(&0u16.to_le_bytes());
        model[at(terms.flags, 2)].copy_from_slice(&0x000Eu16.to_le_bytes());
        let own = vec![0; c.end() as usize];
        let made = LoanContract {
            owner: 1,
            team: 2,
            from: 20340104,
            until: 20340630,
            wage: 90,
            flags: NewLoan::default().flags(),
        };
        let bytes = made.bytes(&model, &own, c, start, terms);
        let u16_at = |offset: u64| u16::from_le_bytes(bytes[at(offset, 2)].try_into().expect("2 bytes"));
        assert_eq!(u16_at(start.agreed_flags), 0);
        // The terms given, and the model's bit that is no term (3), without bit 13.
        assert_eq!(u16_at(terms.flags), 0x0008 | CUP_MATCHES | RECALL);
    }

    #[test]
    fn a_loans_flags_are_in_one_shape_or_the_other() {
        assert_eq!(LoanFlags::of(0x2106, 0x0106, 0x2000), Some(LoanFlags::Agreed(0x0106)));
        assert_eq!(LoanFlags::of(0x0106, 0, 0x2000), Some(LoanFlags::Running(0x0106)));
        // Bit 13 left in a loan whose flags as agreed are cleared, as three English loans of October 2033 hold.
        assert_eq!(LoanFlags::of(0x2106, 0, 0x2000), None);
        assert_eq!(LoanFlags::of(0x0107, 0x0106, 0x2000), None);
    }

    #[test]
    fn loan_terms_read_as_the_loan_club_tab_shows_them() {
        // Anton Kade, from FC Augsburg to Liverpool on the Steam save: cup matches, no recall, cannot be terminated.
        let kade = LoanTerms::decode(0x1002, 23_245_368, 371_665, 725_477).expect("terms");
        assert!(kade.cup_matches && !kade.against_own_club);
        assert!(!kade.can_be_recalled && !kade.can_be_terminated);
        assert_eq!(
            (kade.option_fee, kade.monthly_fee, kade.unused_fee),
            (Some(23_245_368), Some(371_665), Some(725_477))
        );
        // The most common flags there: 0x106, recall allowed, and no amounts.
        let plain = LoanTerms::decode(0x106, -1, -1, -1).expect("terms");
        assert!(plain.can_be_recalled && plain.can_be_terminated && plain.cup_matches);
        assert_eq!((plain.option_fee, plain.monthly_fee, plain.unused_fee), (None, None, None));
        assert_eq!(plain.flags, 0x106);
        // An amount below -1 is no term.
        assert_eq!(LoanTerms::decode(0x106, -2, -1, -1), None);
    }
}
