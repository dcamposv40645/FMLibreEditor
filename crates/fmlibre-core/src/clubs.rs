use std::collections::BTreeMap;

use fmlibre_mem::{Module, Process, is_user_pointer};
use serde::Serialize;

use crate::calibrate::ArrayRef;
use crate::index::{Listed, SearchIndex};
use crate::profile::{ClubFinanceLayout, Layout, StadiumLayout};
use crate::text::read_fm_string;

/// Supporter-mix percentages in the order FM stores them, as the game's Supporter Profile names them.
pub const SUPPORTER_MIX: [&str; 6] = ["hardcore", "core", "family", "fair_weather", "corporate", "casual"];

/// Supporter traits (1–20) in the order FM stores them.
pub const SUPPORTER_TRAITS: [&str; 6] = ["loyalty", "passion", "patience", "affluence", "temperament", "expectations"];

/// The fields shown in club lists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClubSummary {
    pub uid: u32,
    /// Full name, e.g. "Tottenham Hotspur".
    pub name: String,
    pub short_name: Option<String>,
    /// A women's club.
    pub women: bool,
    /// 0–10000, from the senior team.
    pub reputation: Option<u16>,
    pub founded: Option<u16>,
    #[serde(skip)]
    pub address: u64,
}

impl Listed for ClubSummary {
    fn uid(&self) -> u32 {
        self.uid
    }

    fn names(&self) -> String {
        match &self.short_name {
            Some(short) => format!("{} {short}", self.name),
            None => self.name.clone(),
        }
    }

    fn display_name(&self) -> &str {
        &self.name
    }

    fn rank(&self) -> i32 {
        self.reputation.map_or(-1, i32::from)
    }
}

pub type ClubIndex = SearchIndex<ClubSummary>;

/// Reads every club. Entries without a name (a few special records share the array)
/// are skipped. Only reads memory.
pub fn read_clubs(
    process: &Process,
    module: &Module,
    array: &ArrayRef,
    layout: &Layout,
) -> fmlibre_mem::Result<Vec<ClubSummary>> {
    let c = &layout.club;
    let (begin, count) = array.locate(process, module)?;
    let pointers = process.read_bytes(begin, (count * 8) as usize)?;
    let mut buf = vec![0u8; c.end() as usize];

    let mut clubs = Vec::with_capacity(count as usize);
    for chunk in pointers.as_chunks::<8>().0 {
        let object = u64::from_le_bytes(*chunk);
        if !is_user_pointer(object) || process.read_into(object, &mut buf).is_err() {
            continue;
        }
        let u64_at =
            |offset: u64| u64::from_le_bytes(buf[offset as usize..offset as usize + 8].try_into().expect("8 bytes"));
        let Some(name) = read_fm_string(process, u64_at(c.name)) else {
            continue;
        };
        let info = u64_at(c.info);
        clubs.push(ClubSummary {
            uid: u32::from_le_bytes(buf[c.uid as usize..c.uid as usize + 4].try_into().expect("4 bytes")),
            name,
            short_name: read_fm_string(process, u64_at(c.short_name)),
            women: buf[c.women as usize] == 1,
            reputation: senior_team(process, u64_at(c.teams))
                .and_then(|team| read_u16(process, team + layout.team.reputation)),
            founded: is_user_pointer(info)
                .then(|| read_u16(process, info + layout.club_info.founded))
                .flatten()
                .filter(|&year| year > 1800),
            address: object,
        });
    }
    Ok(clubs)
}

/// The first of a club's teams, which is its senior team.
fn senior_team(process: &Process, teams: u64) -> Option<u64> {
    if !is_user_pointer(teams) {
        return None;
    }
    process.read_u64(teams).ok().filter(|&team| is_user_pointer(team))
}

fn read_u16(process: &Process, addr: u64) -> Option<u16> {
    let bytes = process.read_bytes(addr, 2).ok()?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Everything the club profile shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClubDetails {
    #[serde(flatten)]
    pub summary: ClubSummary,
    pub season_tickets: Option<u32>,
    pub social_media: Option<u32>,
    /// Supporter-mix percentages by [`SUPPORTER_MIX`] key; empty when unknown.
    pub supporter_mix: BTreeMap<&'static str, u8>,
    /// Supporter traits (1–20) by [`SUPPORTER_TRAITS`] key; empty when unknown.
    pub supporter_traits: BTreeMap<&'static str, u8>,
    /// training, youth_facilities, youth_coaching, youth_recruitment and, with full
    /// finances, corporate (1–20).
    pub facilities: BTreeMap<&'static str, u8>,
    /// Only clubs with the full finance record have these.
    pub finances: Option<Finances>,
    /// The stadium the senior team plays at, when the layout knows where it is and it looks real.
    pub stadium: Option<Stadium>,
}

/// A stadium's name and capacities, as stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stadium {
    pub name: Option<String>,
    pub capacity: u32,
    pub seating: u32,
    /// The capacity in use when it is limited; 0 when it is not.
    pub used: u32,
    /// The most it can be expanded to.
    pub expansion: u32,
}

/// The largest capacity a stadium's fields are believed to hold; the biggest of the test save held 115,109.
const MOST_SEATS: u32 = 300_000;

/// Whether a stadium's bytes (from its start to [`StadiumLayout::end`]) look like a stadium's: capacities in range,
/// with no more seats than room.
pub(crate) fn stadium_looks_real(bytes: &[u8], s: &StadiumLayout) -> bool {
    let u32_at =
        |offset: u64| u32::from_le_bytes(bytes[offset as usize..offset as usize + 4].try_into().expect("4 bytes"));
    let (capacity, seating) = (u32_at(s.capacity), u32_at(s.seating));
    capacity <= MOST_SEATS && seating <= capacity && u32_at(s.used) <= MOST_SEATS && u32_at(s.expansion) <= MOST_SEATS
}

/// The stadium the senior team of the club at `club` plays at, with its address, when the layout knows where it is
/// and it looks real.
pub(crate) fn club_stadium(process: &Process, layout: &Layout, club: u64) -> Option<(u64, Vec<u8>)> {
    let s = layout.stadium.as_ref()?;
    let teams = process.read_u64(club + layout.club.teams).ok().filter(|&p| is_user_pointer(p))?;
    let team = process.read_u64(teams).ok().filter(|&p| is_user_pointer(p))?;
    if process.read_u64(team + layout.team.club).ok()? != club {
        return None;
    }
    let stadium = process.read_u64(team + s.team_pointer).ok().filter(|&p| is_user_pointer(p))?;
    let bytes = process.read_bytes(stadium, s.end() as usize).ok()?;
    stadium_looks_real(&bytes, s).then_some((stadium, bytes))
}

/// The class (vtable) of stadiums: the one the clubs' stadiums share, when nearly all of them share one. Only objects
/// of this class are written to.
pub(crate) fn stadium_class(process: &Process, module: &Module, array: &ArrayRef, layout: &Layout) -> Option<u64> {
    let (begin, count) = array.locate(process, module).ok()?;
    let pointers = process.read_bytes(begin, (count * 8) as usize).ok()?;
    let mut classes: BTreeMap<u64, usize> = BTreeMap::new();
    let mut found = 0;
    for chunk in pointers.as_chunks::<8>().0 {
        let club = u64::from_le_bytes(*chunk);
        if let Some((_, bytes)) = is_user_pointer(club).then(|| club_stadium(process, layout, club)).flatten() {
            found += 1;
            *classes.entry(u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"))).or_default() += 1;
        }
    }
    let (&class, &count) = classes.iter().max_by_key(|&(_, &count)| count)?;
    (count >= 10 && count * 100 >= found * 99).then_some(class)
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finances {
    /// Pounds.
    pub balance: i32,
    /// Pounds.
    pub transfer_budget: i32,
    /// Pounds per week.
    pub wage_budget: i32,
    /// Pounds per week.
    pub total_wage_budget: i32,
    /// Average match ticket, pounds.
    pub ticket_price: f32,
    /// The share of each player sale's fee the board adds to the transfer budget, 0–100; `None` where the layout does
    /// not place it or it does not read as a percentage.
    pub transfer_revenue: Option<u32>,
}

/// Reads the full profile of `club`; `None` when its object no longer belongs to it.
/// Only reads memory.
pub fn read_club_details(
    process: &Process,
    layout: &Layout,
    club: &ClubSummary,
) -> fmlibre_mem::Result<Option<ClubDetails>> {
    let c = &layout.club;
    let buf = process.read_bytes(club.address, c.end() as usize)?;
    let u64_at =
        |offset: u64| u64::from_le_bytes(buf[offset as usize..offset as usize + 8].try_into().expect("8 bytes"));
    if u32::from_le_bytes(buf[c.uid as usize..c.uid as usize + 4].try_into().expect("4 bytes")) != club.uid {
        return Ok(None);
    }

    let mut details = ClubDetails {
        summary: club.clone(),
        season_tickets: None,
        social_media: None,
        supporter_mix: BTreeMap::new(),
        supporter_traits: BTreeMap::new(),
        facilities: BTreeMap::new(),
        finances: None,
        stadium: None,
    };
    if let (Some(s), Some((_, bytes))) = (layout.stadium.as_ref(), club_stadium(process, layout, club.address)) {
        let u32_at =
            |offset: u64| u32::from_le_bytes(bytes[offset as usize..offset as usize + 4].try_into().expect("4 bytes"));
        let name = u64::from_le_bytes(bytes[s.name as usize..s.name as usize + 8].try_into().expect("8 bytes"));
        details.stadium = Some(Stadium {
            name: read_fm_string(process, name),
            capacity: u32_at(s.capacity),
            seating: u32_at(s.seating),
            used: u32_at(s.used),
            expansion: u32_at(s.expansion),
        });
    }

    let i = &layout.club_info;
    let info = u64_at(c.info);
    if let Some(info) = is_user_pointer(info).then(|| process.read_bytes(info, i.end() as usize).ok()).flatten() {
        let u32_at =
            |offset: u64| u32::from_le_bytes(info[offset as usize..offset as usize + 4].try_into().expect("4 bytes"));
        details.season_tickets = Some(u32_at(i.season_tickets));
        // u32::MAX where the game has no count: most clubs outside the biggest leagues.
        details.social_media = Some(u32_at(i.social_media)).filter(|&n| n != u32::MAX);
        details.supporter_mix =
            SUPPORTER_MIX.iter().enumerate().map(|(n, &key)| (key, info[i.supporter_mix as usize + n])).collect();
        details.supporter_traits =
            SUPPORTER_TRAITS.iter().enumerate().map(|(n, &key)| (key, info[i.supporter_traits as usize + n])).collect();
        for (key, offset) in [
            ("training", i.training),
            ("youth_facilities", i.youth_facilities),
            ("youth_coaching", i.youth_coaching),
            ("youth_recruitment", i.youth_recruitment),
        ] {
            details.facilities.insert(key, info[offset as usize]);
        }
    }

    let f = &layout.club_finance;
    let finance = u64_at(c.finance);
    if let Some(bytes) = is_user_pointer(finance).then(|| process.read_bytes(finance, f.end() as usize).ok()).flatten()
        && finances_look_real(&bytes, f)
    {
        let i32_at =
            |offset: u64| i32::from_le_bytes(bytes[offset as usize..offset as usize + 4].try_into().expect("4 bytes"));
        details.facilities.insert("corporate", bytes[f.corporate as usize]);
        details.finances = Some(Finances {
            balance: i32_at(f.balance),
            transfer_budget: i32_at(f.transfer_budget),
            wage_budget: i32_at(f.wage_budget),
            total_wage_budget: i32_at(f.total_wage_budget),
            ticket_price: f32_at(&bytes, f.ticket_price),
            transfer_revenue: f
                .transfer_revenue
                .and_then(|offset| u32::try_from(i32_at(offset)).ok())
                .filter(|share| *share <= 100),
        });
    }
    Ok(Some(details))
}

fn f32_at(bytes: &[u8], offset: u64) -> f32 {
    f32::from_le_bytes(bytes[offset as usize..offset as usize + 4].try_into().expect("4 bytes"))
}

/// Whether the bytes of a finance record (from its start to [`ClubFinanceLayout::end`])
/// look like a real club's full record. The smaller finance class ends long before the
/// budget fields, so what is read there belongs to something else and looks wrong.
pub(crate) fn finances_look_real(bytes: &[u8], f: &ClubFinanceLayout) -> bool {
    let i32_at =
        |offset: u64| i32::from_le_bytes(bytes[offset as usize..offset as usize + 4].try_into().expect("4 bytes"));
    let ticket_price = f32_at(bytes, f.ticket_price);
    ticket_price.is_finite()
        && (1.0..=1000.0).contains(&ticket_price)
        && (1..=20).contains(&bytes[f.corporate as usize])
        && i32_at(f.transfer_budget) >= -1
        && i32_at(f.wage_budget) >= 0
        && i32_at(f.total_wage_budget) >= 0
}

/// The class (vtable) of the full finance record: the one shared by the clubs whose
/// finances look real, when nearly all of them share one. Only records of this class
/// are long enough to hold the budgets, so only they are ever written.
pub(crate) fn full_finance_class(process: &Process, module: &Module, array: &ArrayRef, layout: &Layout) -> Option<u64> {
    let f = &layout.club_finance;
    let (begin, count) = array.locate(process, module).ok()?;
    let pointers = process.read_bytes(begin, (count * 8) as usize).ok()?;
    let mut classes: BTreeMap<u64, usize> = BTreeMap::new();
    let mut real = 0;
    for chunk in pointers.as_chunks::<8>().0 {
        let club = u64::from_le_bytes(*chunk);
        let Some(finance) = is_user_pointer(club).then(|| process.read_u64(club + layout.club.finance).ok()).flatten()
        else {
            continue;
        };
        if !is_user_pointer(finance) {
            continue;
        }
        let Ok(bytes) = process.read_bytes(finance, f.end() as usize) else {
            continue;
        };
        if finances_look_real(&bytes, f) {
            real += 1;
            *classes.entry(u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"))).or_default() += 1;
        }
    }
    let (&class, &count) = classes.iter().max_by_key(|&(_, &count)| count)?;
    // One class for (nearly) every real-looking record, and more than a handful of them.
    (count >= 10 && count * 100 >= real * 99).then_some(class)
}
