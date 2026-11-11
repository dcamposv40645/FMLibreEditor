//! Changing values in the game: which fields can be written, where they live and
//! which values they take. Every write re-checks whose object it lands in, follows
//! pointers only to records that point back or check out, refuses a value the field
//! cannot hold, refuses to touch a field whose current value does not look like one
//! (the layout would not fit), and reads the value back afterwards.

use std::fmt;
use std::ops::RangeInclusive;

use fmlibre_mem::is_user_pointer;
use serde::{Deserialize, Serialize};

use crate::clubs::{ClubSummary, finances_look_real};
use crate::coded::Coded;
use crate::date::FmDate;
use crate::details::{PERSONALITY, POSITIONS};
use crate::health::Lock;
use crate::players::PlayerSummary;
use crate::profile::{Layout, StadiumLayout};
use crate::session::Session;
use crate::staff::StaffSummary;

/// The kind of object a field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Entity {
    Player,
    Staff,
    Club,
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Entity::Player => "player",
            Entity::Staff => "staff",
            Entity::Club => "club",
        })
    }
}

/// How a value is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    U8,
    I8,
    U16,
    I16,
    /// An amount of money, in pounds.
    I32,
    /// A count, such as a stadium's capacity.
    U32,
    /// A `u16` day of the year (flags above bit 8) and a `u16` year. Values are numbers
    /// like 20290630; 0 is a date the game has not set.
    Date,
    /// One bit of a byte, 0 or 1; the field says which bit. The rest of the byte is left as it is.
    Flag,
    /// A pointer to another of the game's objects, such as a person's nation. Only an object of the same class as
    /// the one it points at now may take its place.
    Pointer,
}

impl Kind {
    pub fn size(self) -> usize {
        match self {
            Kind::U8 | Kind::I8 | Kind::Flag => 1,
            Kind::U16 | Kind::I16 => 2,
            Kind::I32 | Kind::U32 | Kind::Date => 4,
            Kind::Pointer => 8,
        }
    }

    /// The value in `bytes`; a flag's whole byte, as only its field knows which bit is meant.
    pub(crate) fn decode(self, bytes: &[u8]) -> i64 {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        match self {
            Kind::U8 | Kind::Flag => i64::from(bytes[0]),
            Kind::I8 => i64::from(bytes[0] as i8),
            Kind::U16 => i64::from(u16_at(0)),
            Kind::I16 => i64::from(u16_at(0) as i16),
            Kind::I32 => i64::from(i32::from_le_bytes(bytes[..4].try_into().expect("4 bytes"))),
            Kind::U32 => i64::from(u32::from_le_bytes(bytes[..4].try_into().expect("4 bytes"))),
            Kind::Date => FmDate::from_raw(u16_at(0), u16_at(2)).map_or(0, FmDate::to_number),
            Kind::Pointer => i64::from_le_bytes(bytes[..8].try_into().expect("8 bytes")),
        }
    }

    /// The bytes of `value`, which must fit the kind, written over `old`: a date keeps
    /// the flags the game keeps next to its day.
    pub(crate) fn encode(self, value: i64, old: &[u8]) -> Vec<u8> {
        const FITS: &str = "checked against the field's range";
        match self {
            Kind::U8 | Kind::Flag => vec![u8::try_from(value).expect(FITS)],
            Kind::I8 => i8::try_from(value).expect(FITS).to_le_bytes().to_vec(),
            Kind::U16 => u16::try_from(value).expect(FITS).to_le_bytes().to_vec(),
            Kind::I16 => i16::try_from(value).expect(FITS).to_le_bytes().to_vec(),
            Kind::I32 => i32::try_from(value).expect(FITS).to_le_bytes().to_vec(),
            Kind::U32 => u32::try_from(value).expect(FITS).to_le_bytes().to_vec(),
            Kind::Date => {
                let date = FmDate::from_number(value).expect(FITS);
                let flags = u16::from_le_bytes([old[0], old[1]]) & !0x1FF;
                [(flags | date.day_of_year()).to_le_bytes(), date.year.to_le_bytes()].concat()
            }
            Kind::Pointer => value.to_le_bytes().to_vec(),
        }
    }
}

/// The addresses a pointer field may hold: the user half of a 64-bit address space, past the first page.
const POINTERS: RangeInclusive<i64> = 0x1_0000..=0x7FFF_FFFF_FFFF;

/// Why a pointer field refuses a new value; the app has the words in Turkish and Spanish too.
const OTHER_CLASS: &str = "the new value is not one of the game's own records of that kind";

/// How the game shows a stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scale {
    /// As stored.
    Plain,
    /// Stored 0–100, shown 1–20 (see [`crate::Attribute`]).
    Attribute,
    /// Stored 0–10000, shown as a percentage 0–100.
    Percent,
}

impl Scale {
    /// The value the game shows for `stored`.
    pub fn shown(self, stored: i64) -> i64 {
        match self {
            Scale::Plain => stored,
            Scale::Attribute => ((stored + 2) / 5).clamp(1, 20),
            Scale::Percent => (stored + 50) / 100,
        }
    }

    /// A stored value the game shows as `shown`: the middle of the values it rounds to it.
    pub fn stored(self, shown: i64) -> i64 {
        match self {
            Scale::Plain => shown,
            Scale::Attribute => shown * 5,
            Scale::Percent => shown * 100,
        }
    }
}

/// The record a field lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The object itself.
    Object,
    /// The contract of a player or member of staff, which points back at its owner.
    Contract,
    /// The club's info record.
    ClubInfo,
    /// The club's finance record, written only when it is of the full class.
    ClubFinance,
    /// The club's senior team, which points back at the club.
    SeniorTeam,
    /// The stadium the club's senior team plays at, written only when it is of the stadium class.
    Stadium,
    /// The record of a player's reasons to be unhappy, which his contract points at.
    Concerns,
    /// The loan contract of a player on loan (see [`crate::Session::loan`]), written only when it is of the loan
    /// contract class and points back at him.
    Loan,
}

/// A value of a player, member of staff or club that the editor can change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// E.g. `current_ability`, `attribute.finishing`, `contract.wage`, `finance.transfer_budget`.
    pub key: String,
    pub entity: Entity,
    pub kind: Kind,
    pub scale: Scale,
    /// Smallest stored value the editor writes.
    pub min: i64,
    /// Largest stored value the editor writes.
    pub max: i64,
    /// A stored value meaning "none" that the field may be set to besides `min..=max`, such
    /// as 255 for no squad number.
    pub blank: Option<i64>,
    /// The values the field takes, in the order to offer them, when it holds a code from a
    /// list (a squad status) rather than any value of its range.
    pub choices: Option<Vec<i64>>,
    /// Whether a mass edit may change the field; some only make sense one person at a time.
    pub mass: bool,
    /// For a flag, which bit of its byte it is.
    #[serde(skip)]
    bit: u8,
    #[serde(skip)]
    place: Place,
    /// Offset in its record, for the object before any person-block shift.
    #[serde(skip)]
    offset: u64,
    /// Whether the field is in the object's person block and moves with its shift.
    #[serde(skip)]
    shifted: bool,
    /// The stored values a correct layout can produce; anything else means the layout
    /// does not fit this object, and the field is left alone.
    #[serde(skip)]
    plausible: RangeInclusive<i64>,
}

impl Field {
    fn new(entity: Entity, key: impl Into<String>, offset: u64, kind: Kind, range: RangeInclusive<i64>) -> Self {
        Self {
            key: key.into(),
            entity,
            kind,
            scale: Scale::Plain,
            min: *range.start(),
            max: *range.end(),
            blank: None,
            choices: None,
            mass: true,
            bit: 0,
            place: Place::Object,
            offset,
            shifted: false,
            plausible: range,
        }
    }

    /// A place of a club's full finance record that no layout names yet, taking `range` and nothing else, for the
    /// probe to try a place a search found before it is mapped. It is written with the same checks as every field:
    /// the club's object and record, the value now in `range`, and the value read back.
    pub fn unmapped_club_finance(offset: u64, kind: Kind, range: RangeInclusive<i64>) -> Self {
        Field::new(Entity::Club, format!("finance+{offset:#x}"), offset, kind, range)
            .in_record(Place::ClubFinance)
            .single()
    }

    /// A field that is one `bit` of the byte at `offset`: 0 or 1.
    fn flag(entity: Entity, key: impl Into<String>, offset: u64, bit: u8) -> Self {
        Self { bit, ..Field::new(entity, key, offset, Kind::Flag, 0..=1) }
    }

    fn attribute(mut self) -> Self {
        self.scale = Scale::Attribute;
        self
    }

    fn percent(mut self) -> Self {
        self.scale = Scale::Percent;
        self
    }

    /// Lets the field be set to `blank`, which means "none", besides its range.
    fn blank(mut self, blank: i64) -> Self {
        self.blank = Some(blank);
        self
    }

    /// Restricts the field to the codes in `choices`.
    fn choices(mut self, choices: &[i64]) -> Self {
        self.choices = Some(choices.to_vec());
        self
    }

    /// Keeps the field out of mass edits.
    fn single(mut self) -> Self {
        self.mass = false;
        self
    }

    /// Whether the editor writes `value` to this field: one of its range (and of its choices,
    /// when it has them), or its blank.
    pub fn takes(&self, value: i64) -> bool {
        self.blank == Some(value)
            || ((self.min..=self.max).contains(&value) && self.choices.as_ref().is_none_or(|c| c.contains(&value)))
    }

    /// The value of the field in its bytes: for a flag, its bit.
    pub(crate) fn decode(&self, bytes: &[u8]) -> i64 {
        match self.kind {
            Kind::Flag => i64::from((bytes[0] >> self.bit) & 1),
            kind => kind.decode(bytes),
        }
    }

    /// The bytes of `value` written over `old`: a flag keeps the other bits of its byte.
    pub(crate) fn encode(&self, value: i64, old: &[u8]) -> Vec<u8> {
        match self.kind {
            Kind::Flag if value == 0 => vec![old[0] & !(1 << self.bit)],
            Kind::Flag => vec![old[0] | (1 << self.bit)],
            kind => kind.encode(value, old),
        }
    }

    /// Accepts stored values in `plausible` as the game's own, beyond the ones the editor writes.
    fn plausible(mut self, plausible: RangeInclusive<i64>) -> Self {
        self.plausible = plausible;
        self
    }

    fn in_record(mut self, place: Place) -> Self {
        self.place = place;
        self
    }

    fn in_person_block(mut self, person_block: u64) -> Self {
        self.shifted = self.place == Place::Object && self.offset >= person_block;
        self
    }

    /// Whether `value` is one this field can hold in a correctly read object. For a
    /// date that includes a date the game has not set (0).
    pub fn is_plausible(&self, value: i64) -> bool {
        self.plausible.contains(&value)
            && (self.kind != Kind::Date || value == 0 || FmDate::from_number(value).is_some())
    }

    /// Where the field is, given where its record starts.
    fn address(&self, record: u64, target: &Target) -> u64 {
        record + self.offset + if self.shifted { target.shift } else { 0 }
    }
}

/// Every field of `entity` the editor can change, in display order.
pub fn fields(layout: &Layout, entity: Entity) -> Vec<Field> {
    match entity {
        Entity::Player => player_fields(layout),
        Entity::Staff => staff_fields(layout),
        Entity::Club => club_fields(layout),
    }
}

/// A wage in pounds per week, and a contract end between 2000 and 2100.
fn contract_fields(layout: &Layout, entity: Entity) -> [Field; 2] {
    let c = &layout.contract;
    [
        Field::new(entity, "contract.wage", c.wage, Kind::I32, 0..=10_000_000)
            .plausible(0..=i64::from(i32::MAX))
            .in_record(Place::Contract),
        Field::new(entity, "contract.expires", c.expires, Kind::Date, 20000101..=21001231)
            .plausible(0..=22001231)
            .in_record(Place::Contract),
    ]
}

/// The squad statuses the editor offers, by the codes contracts hold them as (see the names in the app's
/// dictionaries). The game uses more codes than these, e.g. for goalkeepers and youngsters.
pub const SQUAD_STATUSES: [i64; 5] = [1, 2, 3, 4, 5];

/// A squad number from 1 to 99 (255: none), the squad status, and whether the player is on
/// the transfer list and on the loan list: one player at a time.
fn player_contract_fields(layout: &Layout) -> [Field; 4] {
    let c = &layout.contract;
    let field = |key: &str, offset: u64, range: RangeInclusive<i64>| {
        Field::new(Entity::Player, key, offset, Kind::U8, range).single().in_record(Place::Contract)
    };
    [
        field("contract.squad_number", c.squad_number, 1..=99).plausible(1..=255).blank(NO_SQUAD_NUMBER),
        field("contract.squad_status", c.squad_status, 1..=40).plausible(0..=40).choices(&SQUAD_STATUSES),
        Field::flag(Entity::Player, "contract.transfer_listed", c.listing, 0).single().in_record(Place::Contract),
        Field::flag(Entity::Player, "contract.loan_listed", c.listing, 1).single().in_record(Place::Contract),
    ]
}

/// The loyalty bonus, and how happy the player is and how happy with his playing time, as FMRTE shows them (the
/// game shows them in words). Happiness ran from -40 to 97 on the test save.
fn player_mood_fields(layout: &Layout) -> [Field; 4] {
    let c = &layout.contract;
    let mood = |key: &str, offset: u64| {
        Field::new(Entity::Player, key, offset, Kind::I8, -100..=100).plausible(-128..=127).in_record(Place::Contract)
    };
    [
        Field::new(Entity::Player, "contract.loyalty_bonus", c.loyalty_bonus, Kind::I32, 0..=10_000_000)
            .plausible(0..=i64::from(i32::MAX))
            .in_record(Place::Contract),
        mood("contract.happiness", c.happiness),
        mood("contract.playing_time_happiness", c.playing_time_happiness),
        // How many reasons to be unhappy his record lists: only ever set to none, as FMRTE's "Remove all" does. The
        // entries stay where they are, so an undo lists them again.
        Field::new(Entity::Player, "contract.unhappiness", 0, Kind::U32, 0..=0)
            .plausible(0..=crate::details::MOST_CONCERNS as i64)
            .in_record(Place::Concerns),
    ]
}

/// The squad number of a player without one.
pub(crate) const NO_SQUAD_NUMBER: i64 = 255;

/// The field of the day a player's loan ends.
pub const LOAN_EXPIRES: &str = "loan.expires";

/// The field of the fee for which the club a player is on loan at may sign him for good: the loan's option to buy,
/// pounds, -1 for none.
pub const LOAN_OPTION_FEE: &str = "loan.option_fee";

fn player_fields(layout: &Layout) -> Vec<Field> {
    let p = &layout.player;
    let field = |key: &str, offset: u64, kind: Kind, range: RangeInclusive<i64>| {
        Field::new(Entity::Player, key, offset, kind, range)
    };
    let mut out = vec![
        field("current_ability", p.current_ability, Kind::U16, 1..=200),
        // Database ranges are negative; the game settles them when a save starts.
        field("potential_ability", p.potential_ability, Kind::I16, 1..=200).plausible(-200..=200),
        field("height", p.height, Kind::U16, 140..=220).plausible(100..=250),
        field("nation", p.nation, Kind::Pointer, POINTERS).single(),
        // Any birth date from 1940 to 2020; the game works out the age from it.
        field("date_of_birth", p.date_of_birth, Kind::Date, 19400101..=20201231).plausible(19000101..=21001231),
        // Pounds; -1 is none set.
        field("asking_price", p.asking_price, Kind::I32, 0..=2_000_000_000)
            .plausible(-1..=i64::from(i32::MAX))
            .blank(-1),
        field("reputation.home", p.home_reputation, Kind::U16, 0..=10_000),
        field("reputation.current", p.current_reputation, Kind::U16, 0..=10_000),
        field("reputation.world", p.world_reputation, Kind::U16, 0..=10_000),
        field("international.caps", p.international_caps, Kind::U16, 0..=1_000),
        field("international.goals", p.international_goals, Kind::U16, 0..=1_000),
        field("international.u21_caps", p.u21_caps, Kind::U8, 0..=255),
        field("international.u21_goals", p.u21_goals, Kind::U8, 0..=255),
        // What the game changes every day: energy, match fitness, jadedness and morale. No player on
        // the test save had less than 35% match sharpness, so the editor does not write less either;
        // it accepts less as it is, as players the game creates (youth intakes) start below that.
        // Morale ran from 1 to 20. Jadedness is -500 for a rested player, the least the game keeps; the
        // most seen was 252, early in a season, so the editor writes up to 1000 and accepts any
        // value of a worn-out player as it is.
        field("condition", p.condition, Kind::U16, 0..=10_000).percent(),
        field("match_sharpness", p.match_sharpness, Kind::U16, 3_500..=10_000).plausible(0..=10_000).percent(),
        field("jadedness", p.jadedness, Kind::I16, -500..=1_000).plausible(-500..=i64::from(i16::MAX)),
        field("morale", p.morale, Kind::U8, 1..=20),
    ];
    out.extend(crate::ATTRIBUTE_GROUPS.iter().flat_map(|(_, keys)| keys.iter()).filter_map(|key| {
        let offset = *p.attributes.get(*key)?;
        Some(field(&format!("attribute.{key}"), offset, Kind::U8, 1..=100).plausible(0..=100).attribute())
    }));
    // The game no longer plays a sweeper: every player rates 1 there, so it is not offered.
    out.extend(
        POSITIONS.iter().enumerate().filter(|&(_, &key)| key != "sw").map(|(i, key)| {
            field(&format!("position.{key}"), p.positions + i as u64, Kind::U8, 1..=20).plausible(0..=20)
        }),
    );
    out.extend(PERSONALITY.iter().enumerate().map(|(i, key)| {
        field(&format!("personality.{key}"), p.personality + i as u64, Kind::U8, 1..=20).plausible(0..=20)
    }));
    out.extend(contract_fields(layout, Entity::Player));
    out.extend(player_contract_fields(layout));
    out.extend(player_mood_fields(layout));
    // The day his loan ends, in his loan contract: one player at a time, and only to the game's date or a later one (see
    // `Session::write`). The game takes it as the loan's last day: when that day is over it ends the loan itself.
    if let Some(loan) = &layout.loan {
        out.push(
            Field::new(Entity::Player, LOAN_EXPIRES, layout.contract.expires, Kind::Date, 20000101..=21001231)
                .plausible(0..=22001231)
                .single()
                .in_record(Place::Loan),
        );
        // The loan's option to buy, pounds; -1 is none. When a loan with one ends, the game may let the club he is on
        // loan at sign him for it: Anton Kade, recalled by the editor, went to Liverpool for his £23,245,368 option the
        // day his loan ended. So a recall takes it away (see the app's `recall_loan`).
        if let Some(terms) = &loan.terms {
            out.push(
                Field::new(Entity::Player, LOAN_OPTION_FEE, terms.option_fee, Kind::I32, 0..=2_000_000_000)
                    .plausible(-1..=i64::from(i32::MAX))
                    .blank(-1)
                    .single()
                    .in_record(Place::Loan),
            );
        }
    }
    // His traits: one flag each, `trait.0` to `trait.63`, bit n of the mask.
    out.extend((0..64u64).map(|n| Field::flag(Entity::Player, format!("trait.{n}"), p.traits + n / 8, (n % 8) as u8)));
    out.into_iter().map(|f| f.in_person_block(p.person_block)).collect()
}

fn staff_fields(layout: &Layout) -> Vec<Field> {
    let s = &layout.staff;
    let field = |key: &str, offset: u64, kind: Kind, range: RangeInclusive<i64>| {
        Field::new(Entity::Staff, key, offset, kind, range)
    };
    let mut out = vec![
        field("current_ability", s.current_ability, Kind::U16, 1..=200).plausible(0..=200),
        field("potential_ability", s.potential_ability, Kind::I16, 1..=200).plausible(-200..=200),
        field("reputation.home", s.home_reputation, Kind::U16, 0..=10_000),
        field("reputation.current", s.current_reputation, Kind::U16, 0..=10_000),
        field("reputation.world", s.world_reputation, Kind::U16, 0..=10_000),
        field("nation", s.nation, Kind::Pointer, POINTERS).single(),
    ];
    out.extend(crate::STAFF_ATTRIBUTE_GROUPS.iter().flat_map(|(_, keys)| keys.iter()).filter_map(|key| {
        if let Some(&offset) = s.attributes.get(*key) {
            Some(field(&format!("attribute.{key}"), offset, Kind::U8, 0..=100).attribute())
        } else {
            let offset = *s.ratings.get(*key)?;
            Some(field(&format!("attribute.{key}"), offset, Kind::U8, 1..=20).plausible(0..=20))
        }
    }));
    out.extend(PERSONALITY.iter().enumerate().map(|(i, key)| {
        field(&format!("personality.{key}"), s.personality + i as u64, Kind::U8, 1..=20).plausible(0..=20)
    }));
    out.extend(contract_fields(layout, Entity::Staff));
    out.into_iter().map(|f| f.in_person_block(s.person_block)).collect()
}

fn club_fields(layout: &Layout) -> Vec<Field> {
    let (i, f) = (&layout.club_info, &layout.club_finance);
    let field = |key: &str, offset: u64, kind: Kind, range: RangeInclusive<i64>, place: Place| {
        Field::new(Entity::Club, key, offset, kind, range).in_record(place)
    };
    let facility = |key: &str, offset: u64, place: Place| field(key, offset, Kind::U8, 1..=20, place).plausible(0..=20);
    let money = |key: &str, offset: u64, range: RangeInclusive<i64>| {
        field(key, offset, Kind::I32, range, Place::ClubFinance).plausible(i64::from(i32::MIN)..=i64::from(i32::MAX))
    };
    let mut fields = vec![
        field("reputation", layout.team.reputation, Kind::U16, 0..=10_000, Place::SeniorTeam),
        facility("facilities.training", i.training, Place::ClubInfo),
        facility("facilities.youth_facilities", i.youth_facilities, Place::ClubInfo),
        facility("facilities.youth_coaching", i.youth_coaching, Place::ClubInfo),
        facility("facilities.youth_recruitment", i.youth_recruitment, Place::ClubInfo),
        facility("facilities.corporate", f.corporate, Place::ClubFinance),
        money("finance.balance", f.balance, -2_000_000_000..=2_000_000_000),
        money("finance.transfer_budget", f.transfer_budget, 0..=2_000_000_000),
        money("finance.wage_budget", f.wage_budget, 0..=100_000_000),
        money("finance.total_wage_budget", f.total_wage_budget, 0..=100_000_000),
    ];
    // A percentage the game keeps in an i32: as a u32 it writes the same bytes for 0..=100, and a value that does not
    // read as one leaves the field alone.
    if let Some(offset) = f.transfer_revenue {
        fields.push(field("finance.transfer_revenue", offset, Kind::U32, 0..=100, Place::ClubFinance));
    }
    // Capacities, as stored. The biggest stadium of the test save held 115,109.
    if let Some(s) = &layout.stadium {
        let seats = |key: &str, offset: u64| {
            field(key, offset, Kind::U32, 0..=250_000, Place::Stadium).plausible(0..=i64::from(i32::MAX))
        };
        fields.extend([
            seats("stadium.capacity", s.capacity),
            seats("stadium.seating", s.seating),
            seats("stadium.used", s.used),
            seats("stadium.expansion", s.expansion),
        ]);
    }
    fields
}

/// What `field` of a stadium that holds `capacity` and seats `seating` may be set to, within `(min, max)`, so that it
/// still seats no more than it holds (see `crate::clubs::stadium_looks_real`): the seating up to the capacity, the
/// capacity down to the seating. `None` for the stadium's other fields, which nothing ties to the rest.
fn stadium_limits(
    stadium: &StadiumLayout,
    field: &Field,
    (min, max): (i64, i64),
    capacity: i64,
    seating: i64,
) -> Option<(i64, i64)> {
    if field.offset == stadium.seating {
        Some((min, max.min(capacity)))
    } else if field.offset == stadium.capacity {
        Some((min.max(seating), max))
    } else {
        None
    }
}

/// The object an edit lands in, as found when the lists were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub entity: Entity,
    pub uid: u32,
    pub address: u64,
    /// The object's person-block shift; 0 for clubs.
    pub shift: u64,
}

impl PlayerSummary {
    pub fn target(&self) -> Target {
        Target { entity: Entity::Player, uid: self.uid, address: self.address, shift: self.shift }
    }

    /// Brings the fields a list row shows, and is filtered by, up to date after `field` was set to `value`; an
    /// attribute's value as FM shows it (1–20).
    pub fn note_change(&mut self, field: &str, value: i64) {
        if let Some(key) = field.strip_prefix("attribute.") {
            if let Some(slot) = crate::filter::player_attribute(key).and_then(|i| self.attributes.get_mut(i)) {
                *slot = value as u8;
            }
            return;
        }
        match field {
            "current_ability" => self.current_ability = value as u16,
            "potential_ability" => self.potential_ability = value as i16,
            "contract.expires" => self.contract_expires = FmDate::from_number(value),
            // The age follows from the game's date, which the caller has (see `note_birth`).
            "date_of_birth" => self.date_of_birth = FmDate::from_number(value),
            // The nation's name and UID follow from the app's list of nations (see `crate::nations`).
            "nation" => self.nation_address = value as u64,
            "contract.transfer_listed" => self.transfer_listed = value != 0,
            "contract.loan_listed" => self.loan_listed = value != 0,
            _ => {
                let position = field.strip_prefix("position.").and_then(|key| POSITIONS.iter().position(|&k| k == key));
                if let Some(i) = position {
                    self.position_ratings[i] = value as u8;
                    self.positions = crate::players::natural_positions(&self.position_ratings);
                }
            }
        }
    }
}

impl PlayerSummary {
    /// Works the age out again on `today`, the game's date, after the birth date changed.
    pub fn note_birth(&mut self, today: Option<FmDate>) {
        self.age = self.date_of_birth.zip(today).and_then(|(born, today)| born.age_on(today));
    }
}

impl StaffSummary {
    pub fn target(&self) -> Target {
        Target { entity: Entity::Staff, uid: self.uid, address: self.address, shift: self.shift }
    }

    /// Brings the fields a list row shows, and is filtered by, up to date after `field` was set to `value`; an
    /// attribute's value as FM shows it (1–20).
    pub fn note_change(&mut self, field: &str, value: i64) {
        if let Some(key) = field.strip_prefix("attribute.") {
            if let Some(slot) = crate::filter::staff_attribute(key).and_then(|i| self.attributes.get_mut(i)) {
                *slot = value as u8;
            }
            return;
        }
        match field {
            "current_ability" => self.current_ability = value as u16,
            "potential_ability" => self.potential_ability = value as i16,
            "contract.expires" => self.contract_expires = FmDate::from_number(value),
            "nation" => self.nation_address = value as u64,
            _ => {}
        }
    }
}

impl ClubSummary {
    pub fn target(&self) -> Target {
        Target { entity: Entity::Club, uid: self.uid, address: self.address, shift: 0 }
    }

    /// Brings the fields a list row shows up to date after `field` was set to `value`.
    pub fn note_change(&mut self, field: &str, value: i64) {
        if field == "reputation" {
            self.reputation = Some(value as u16);
        }
    }
}

/// A value the editor changed: enough to show it in a change log and to undo it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub entity: Entity,
    pub uid: u32,
    pub field: String,
    pub old: i64,
    pub new: i64,
}

#[derive(Debug)]
pub enum EditError {
    UnknownField {
        entity: Entity,
        key: String,
    },
    OutOfRange {
        field: String,
        value: i64,
        min: i64,
        max: i64,
    },
    /// A date field was given a number that is not a date.
    NotADate {
        field: String,
        value: i64,
    },
    /// A field that holds a code from a list was given a code the editor does not offer.
    NotAChoice {
        field: String,
        value: i64,
    },
    /// Another player of the team already has this squad number.
    Taken {
        field: String,
        value: i64,
        /// The UID of the player who has it.
        holder: u32,
    },
    /// The field cannot be changed this way, e.g. a date by adding to it.
    Unsupported {
        field: String,
        reason: &'static str,
    },
    /// The object no longer belongs to the person or club: the game has moved on.
    Moved {
        entity: Entity,
        uid: u32,
    },
    /// The record the field lives in is missing or does not check out, e.g. no
    /// contract, or a club without full finances.
    NoRecord {
        field: String,
    },
    /// The game has not set this date, so it is left alone.
    Unset {
        field: String,
    },
    /// The game holds a value the field cannot have, so the layout does not fit this object.
    Implausible {
        field: String,
        value: i64,
    },
    /// The value read back is not the one written.
    NotApplied {
        field: String,
        wrote: i64,
        read: i64,
    },
    /// The value has changed in the game since the change being undone was made.
    Conflict {
        field: String,
        expected: i64,
        found: i64,
    },
    /// The editor does not write this field to this game: see [`Lock`].
    Locked {
        field: String,
        lock: Lock,
    },
    Memory(fmlibre_mem::Error),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::UnknownField { entity, key } => write!(f, "{entity} has no editable field {key}"),
            EditError::OutOfRange { field, value, min, max } => {
                write!(f, "{value} is not a valid {field}; it takes {min} to {max}")
            }
            EditError::NotADate { field, value } => write!(f, "{value} is not a date for {field}"),
            EditError::NotAChoice { field, value } => write!(f, "{value} is not one of the values {field} takes"),
            EditError::Taken { field, value, holder } => {
                write!(f, "{field} {value} is taken: the player with UID {holder} in the same team has it")
            }
            EditError::Unsupported { field, reason } => write!(f, "{field}: {reason}"),
            EditError::Moved { entity, uid } => {
                write!(f, "the {entity} with id {uid} has moved in the game's memory; reload and try again")
            }
            EditError::NoRecord { field } => {
                write!(f, "{field} cannot be changed here: its record is missing or does not check out")
            }
            EditError::Unset { field } => write!(f, "{field} is not set in the game, so it is left alone"),
            EditError::Implausible { field, value } => {
                write!(f, "{field} reads {value}, which it cannot hold; left unchanged to be safe")
            }
            EditError::NotApplied { field, wrote, read } => {
                write!(f, "wrote {wrote} to {field} but read back {read}")
            }
            EditError::Conflict { field, expected, found } => {
                write!(f, "{field} is now {found}, not {expected}; the game has changed it since")
            }
            EditError::Locked { field, lock } => write!(f, "{field} is left unchanged: {lock}"),
            EditError::Memory(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for EditError {}

impl Coded for EditError {
    fn code(&self) -> &'static str {
        match self {
            EditError::UnknownField { .. } => "edit.unknownField",
            EditError::OutOfRange { .. } => "edit.outOfRange",
            EditError::NotADate { .. } => "edit.notADate",
            EditError::NotAChoice { .. } => "edit.notAChoice",
            EditError::Taken { .. } => "edit.taken",
            EditError::Unsupported { .. } => "edit.unsupported",
            EditError::Moved { .. } => "edit.moved",
            EditError::NoRecord { .. } => "edit.noRecord",
            EditError::Unset { .. } => "edit.unset",
            EditError::Implausible { .. } => "edit.implausible",
            EditError::NotApplied { .. } => "edit.notApplied",
            EditError::Conflict { .. } => "edit.conflict",
            EditError::Locked { lock, .. } => lock.code(),
            EditError::Memory(err) => err.code(),
        }
    }

    fn params(&self) -> Vec<(&'static str, String)> {
        let field = |key: &String| ("field", key.clone());
        let number = |name: &'static str, value: &i64| (name, value.to_string());
        match self {
            EditError::UnknownField { entity, key } => vec![("entity", entity.to_string()), field(key)],
            EditError::OutOfRange { field: key, value, min, max } => {
                vec![field(key), number("value", value), number("min", min), number("max", max)]
            }
            EditError::NotADate { field: key, value }
            | EditError::NotAChoice { field: key, value }
            | EditError::Implausible { field: key, value } => vec![field(key), number("value", value)],
            EditError::Taken { field: key, value, holder } => {
                vec![field(key), number("value", value), ("uid", holder.to_string())]
            }
            EditError::Unsupported { field: key, reason } => vec![field(key), ("reason", (*reason).to_owned())],
            EditError::Moved { entity, uid } => vec![("entity", entity.to_string()), ("uid", uid.to_string())],
            EditError::NoRecord { field: key } | EditError::Unset { field: key } => vec![field(key)],
            EditError::NotApplied { field: key, wrote, read } => {
                vec![field(key), number("wrote", wrote), number("read", read)]
            }
            EditError::Conflict { field: key, expected, found } => {
                vec![field(key), number("expected", expected), number("found", found)]
            }
            EditError::Locked { field: key, lock } => [vec![field(key)], lock.params()].concat(),
            EditError::Memory(err) => err.params(),
        }
    }
}

impl From<fmlibre_mem::Error> for EditError {
    fn from(err: fmlibre_mem::Error) -> Self {
        EditError::Memory(err)
    }
}

impl Session {
    /// The fields of `entity` the editor can change.
    pub fn fields(&self, entity: Entity) -> Vec<Field> {
        fields(&self.layout, entity)
    }

    /// The field of `entity` called `key`.
    pub fn field(&self, entity: Entity, key: &str) -> Result<Field, EditError> {
        self.fields(entity)
            .into_iter()
            .find(|f| f.key == key)
            .ok_or_else(|| EditError::UnknownField { entity, key: key.to_owned() })
    }

    /// Reads `field` of `target`, after checking the object still belongs to it.
    pub fn read_field(&self, target: &Target, field: &Field) -> Result<i64, EditError> {
        self.check_target(target, field)?;
        let address = self.locate(target, field)?.ok_or_else(|| EditError::NoRecord { field: field.key.clone() })?;
        Ok(field.decode(&self.process.read_bytes(address, field.kind.size())?))
    }

    /// Reads every one of `fields` of `target`: `None` for a field whose record the
    /// target lacks (no contract, a club without full finances).
    pub fn read_fields(&self, target: &Target, fields: &[Field]) -> Result<Vec<Option<i64>>, EditError> {
        for field in fields {
            if field.entity != target.entity {
                return Err(EditError::UnknownField { entity: target.entity, key: field.key.clone() });
            }
        }
        self.check_uid(target)?;
        // The fields in the object itself come from a single read of it.
        let in_object = |field: &&Field| field.place == Place::Object;
        let local = |field: &Field| (field.address(target.address, target) - target.address) as usize;
        let len = fields.iter().filter(in_object).map(|field| local(field) + field.kind.size()).max().unwrap_or(0);
        let object = self.process.read_bytes(target.address, len)?;
        let mut records: Vec<(Place, Option<u64>)> = Vec::new();
        let mut out = Vec::with_capacity(fields.len());
        for field in fields {
            if field.place == Place::Object {
                out.push(Some(field.decode(&object[local(field)..])));
                continue;
            }
            let record = match records.iter().find(|(place, _)| *place == field.place) {
                Some(&(_, record)) => record,
                None => {
                    let record = self.record(target, field.place)?;
                    records.push((field.place, record));
                    record
                }
            };
            out.push(match record {
                Some(record) => {
                    let bytes = self.process.read_bytes(field.address(record, target), field.kind.size())?;
                    Some(field.decode(&bytes))
                }
                None => None,
            });
        }
        Ok(out)
    }

    /// Writes `value` (as stored) to `field` of `target` and reads it back.
    ///
    /// Nothing is written unless `value` is in the field's range, the object still
    /// belongs to the target, the field's record checks out and its current value is
    /// plausible. If the value read back differs, the old bytes are written back before
    /// the error is returned.
    pub fn write_field(&self, target: &Target, field: &Field, value: i64) -> Result<Change, EditError> {
        self.write(target, field, value, None, false)
    }

    /// Like [`Session::write_field`], but only while the game still holds `expected`:
    /// otherwise nothing is written and the error is a [`EditError::Conflict`].
    pub fn write_field_if(
        &self,
        target: &Target,
        field: &Field,
        expected: i64,
        value: i64,
    ) -> Result<Change, EditError> {
        self.write(target, field, value, Some(expected), false)
    }

    /// Writes `value`. A value being restored (`restoring`) was the game's own, so it only
    /// has to be plausible, not in the range the editor writes: a potential ability the
    /// database left negative goes back as it was.
    fn write(
        &self,
        target: &Target,
        field: &Field,
        value: i64,
        expected: Option<i64>,
        restoring: bool,
    ) -> Result<Change, EditError> {
        // Putting back what was there is always allowed; anything else only where the checks allow it.
        if !restoring && let Some(lock) = self.field_lock(field) {
            return Err(EditError::Locked { field: field.key.clone(), lock });
        }
        let allowed = if restoring { field.is_plausible(value) } else { field.takes(value) };
        if !allowed {
            if field.choices.is_some() && (field.min..=field.max).contains(&value) {
                return Err(EditError::NotAChoice { field: field.key.clone(), value });
            }
            return Err(EditError::OutOfRange { field: field.key.clone(), value, min: field.min, max: field.max });
        }
        if field.kind == Kind::Date && FmDate::from_number(value).is_none() {
            return Err(EditError::NotADate { field: field.key.clone(), value });
        }
        // A loan ends on a day the game is yet to finish: the end of a day it has finished would never come, and with it
        // neither would the end of the loan.
        if field.key == LOAN_EXPIRES && !restoring {
            let today = self.loan_recall_day()?.to_number();
            if value < today {
                return Err(EditError::OutOfRange { field: field.key.clone(), value, min: today, max: field.max });
            }
        }
        self.check_target(target, field)?;
        let record =
            self.record(target, field.place)?.ok_or_else(|| EditError::NoRecord { field: field.key.clone() })?;
        let address = field.address(record, target);
        let old_bytes = self.process.read_bytes(address, field.kind.size())?;
        let old = field.decode(&old_bytes);
        if let Some(expected) = expected
            && old != expected
        {
            return Err(EditError::Conflict { field: field.key.clone(), expected, found: old });
        }
        if !field.is_plausible(old) {
            return Err(EditError::Implausible { field: field.key.clone(), value: old });
        }
        if field.kind == Kind::Date && old == 0 {
            return Err(EditError::Unset { field: field.key.clone() });
        }
        let change = Change { entity: target.entity, uid: target.uid, field: field.key.clone(), old, new: value };
        if old == value {
            return Ok(change);
        }
        if field.kind == Kind::Pointer && !self.same_class(old, value)? {
            return Err(EditError::Unsupported { field: field.key.clone(), reason: OTHER_CLASS });
        }
        // A stadium seats no more than it holds. A seating above its capacity, or a capacity below its seating, would
        // leave a record the editor no longer takes for a stadium: neither shown nor changed again, not even undone.
        if let (Place::Stadium, Some(stadium)) = (field.place, &self.layout.stadium) {
            let seats = |offset: u64| -> Result<i64, EditError> { Ok(self.process.read_u32(record + offset)?.into()) };
            let bounds = if restoring { field.plausible.clone().into_inner() } else { (field.min, field.max) };
            if let Some((min, max)) =
                stadium_limits(stadium, field, bounds, seats(stadium.capacity)?, seats(stadium.seating)?)
                && !(min..=max).contains(&value)
            {
                return Err(EditError::OutOfRange { field: field.key.clone(), value, min, max });
            }
        }
        // Two players of a team on the pitch with one number would confuse the game; the players it has lent out
        // keep theirs, so they do not count.
        if field.key == "contract.squad_number"
            && !restoring
            && field.blank != Some(value)
            && let Some(holder) = self.squad_number_holder(target, value)?
        {
            return Err(EditError::Taken { field: field.key.clone(), value, holder });
        }
        self.process.write_bytes(address, &field.encode(value, &old_bytes))?;
        let read = field.decode(&self.process.read_bytes(address, field.kind.size())?);
        if read != value {
            // Best effort: put back what was there.
            let _ = self.process.write_bytes(address, &old_bytes);
            return Err(EditError::NotApplied { field: field.key.clone(), wrote: value, read });
        }
        Ok(change)
    }

    /// Whether the objects at `a` and `b` are of one class: their first word, the class's table, is the same.
    fn same_class(&self, a: i64, b: i64) -> Result<bool, EditError> {
        if !is_user_pointer(b as u64) {
            return Ok(false);
        }
        Ok(self.process.read_u64(a as u64)? == self.process.read_u64(b as u64)?)
    }

    /// The UID of another player of the team the player `target` plays for who wears the
    /// squad number `number`, among the players who play for that team now.
    fn squad_number_holder(&self, target: &Target, number: i64) -> Result<Option<u32>, EditError> {
        let (p, c) = (&self.layout.player, &self.layout.contract);
        let team = self.process.read_u64(target.address + p.team)?;
        if !is_user_pointer(team) {
            return Ok(None);
        }
        let header = self.process.read_bytes(team + self.layout.team.players, 16)?;
        let begin = u64::from_le_bytes(header[..8].try_into().expect("8 bytes"));
        let end = u64::from_le_bytes(header[8..].try_into().expect("8 bytes"));
        if !is_user_pointer(begin) || end <= begin || end - begin > 8 * 2_000 {
            return Ok(None);
        }
        for element in self.process.read_bytes(begin, (end - begin) as usize)?.as_chunks::<8>().0 {
            let player = u64::from_le_bytes(*element);
            if player == target.address || self.process.read_u64(player + p.team)? != team {
                continue;
            }
            let shift = crate::players::person_shift(&self.process, p, player);
            let contract = self.process.read_u64(player + p.at(p.contract, shift))?;
            if is_user_pointer(contract)
                && i64::from(self.process.read_bytes(contract + c.squad_number, 1)?[0]) == number
            {
                return Ok(Some(self.process.read_u32(player + p.at(p.uid, shift))?));
            }
        }
        Ok(None)
    }

    /// The first day a loan may be made to end on, the game's date: a loan recalled now ends today. The game takes a
    /// loan's end as its last day, so it ends the loan when today is over, and the player is back at his club the next
    /// day: one day on. (A loan made to end tomorrow brought the players back two days on.)
    pub fn loan_recall_day(&self) -> Result<FmDate, EditError> {
        self.game_date()
            .ok_or(EditError::Unsupported { field: LOAN_EXPIRES.to_owned(), reason: "the in-game date is not known" })
    }

    /// Undoes `change`, provided the game still holds the value it wrote.
    pub fn revert(&self, target: &Target, change: &Change) -> Result<Change, EditError> {
        let field = self.field(target.entity, &change.field)?;
        self.write(target, &field, change.old, Some(change.new), true)
    }

    fn check_target(&self, target: &Target, field: &Field) -> Result<(), EditError> {
        if field.entity != target.entity {
            return Err(EditError::UnknownField { entity: target.entity, key: field.key.clone() });
        }
        self.check_uid(target)
    }

    /// Checks that the object at the target's address still carries the target's UID.
    fn check_uid(&self, target: &Target) -> Result<(), EditError> {
        let uid_offset = match target.entity {
            Entity::Player => self.layout.player.at(self.layout.player.uid, target.shift),
            Entity::Staff => self.layout.staff.at(self.layout.staff.uid, target.shift),
            Entity::Club => self.layout.club.uid,
        };
        if self.process.read_u32(target.address + uid_offset)? != target.uid {
            return Err(EditError::Moved { entity: target.entity, uid: target.uid });
        }
        Ok(())
    }

    /// Where `field` of `target` is, or `None` when its record is missing or does not check out.
    fn locate(&self, target: &Target, field: &Field) -> Result<Option<u64>, EditError> {
        Ok(self.record(target, field.place)?.map(|record| field.address(record, target)))
    }

    /// Where the record `place` of `target` starts, following its pointer, or `None`
    /// when the target has none or it does not check out.
    fn record(&self, target: &Target, place: Place) -> Result<Option<u64>, EditError> {
        let pointer = |offset: u64| -> Result<Option<u64>, EditError> {
            let value = self.process.read_u64(target.address + offset)?;
            Ok(is_user_pointer(value).then_some(value))
        };
        let club = &self.layout.club;
        Ok(match (place, target.entity) {
            (Place::Object, _) => Some(target.address),
            (Place::Contract, Entity::Player | Entity::Staff) => {
                let (contract, identity) = match target.entity {
                    Entity::Player => (self.layout.player.contract, self.layout.player.identity()),
                    _ => (self.layout.staff.contract, self.layout.staff.identity()),
                };
                // Both the pointer and the identity block it must point back at move with the shift.
                let Some(record) = pointer(contract + target.shift)? else {
                    return Ok(None);
                };
                let owner = self.process.read_u64(record + self.layout.contract.owner)?;
                (owner == target.address + identity + target.shift).then_some(record)
            }
            (Place::ClubInfo, Entity::Club) => pointer(club.info)?,
            (Place::ClubFinance, Entity::Club) => {
                let Some(record) = pointer(club.finance)? else {
                    return Ok(None);
                };
                let bytes = self.process.read_bytes(record, self.layout.club_finance.end() as usize)?;
                let class = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
                let full = self.finance_class() == Some(class) && finances_look_real(&bytes, &self.layout.club_finance);
                full.then_some(record)
            }
            (Place::SeniorTeam, Entity::Club) => {
                let Some(teams) = pointer(club.teams)? else {
                    return Ok(None);
                };
                let team = self.process.read_u64(teams)?;
                let belongs =
                    is_user_pointer(team) && self.process.read_u64(team + self.layout.team.club)? == target.address;
                belongs.then_some(team)
            }
            (Place::Concerns, Entity::Player) => {
                let Some(contract) = self.record(target, Place::Contract)? else {
                    return Ok(None);
                };
                let record = self.process.read_u64(contract + self.layout.contract.unhappiness)?;
                is_user_pointer(record).then_some(record)
            }
            (Place::Stadium, Entity::Club) => {
                let Some((stadium, bytes)) = crate::clubs::club_stadium(&self.process, &self.layout, target.address)
                else {
                    return Ok(None);
                };
                let class = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
                (self.stadium_class() == Some(class)).then_some(stadium)
            }
            (Place::Loan, Entity::Player) => self.loan_contract(target.address, target.shift)?,
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip() {
        let old = [0u8; 4];
        for (kind, value) in [
            (Kind::U8, 200),
            (Kind::U16, 10_000),
            (Kind::I16, -95),
            (Kind::I16, 200),
            (Kind::I32, -1_500_000),
            (Kind::U32, 250_000),
        ] {
            assert_eq!(kind.decode(&kind.encode(value, &old)), value);
            assert_eq!(kind.encode(value, &old).len(), kind.size());
        }
    }

    #[test]
    fn dates_keep_the_game_s_flags() {
        // 31 Dec 2028 with a flag above the day, as the game may store it.
        let old = [(0x8000u16 | 366).to_le_bytes(), 2028u16.to_le_bytes()].concat();
        assert_eq!(Kind::Date.decode(&old), 20281231);
        let new = Kind::Date.encode(20290630, &old);
        assert_eq!(u16::from_le_bytes([new[0], new[1]]), 0x8000 | 181);
        assert_eq!(u16::from_le_bytes([new[2], new[3]]), 2029);
        assert_eq!(Kind::Date.decode(&new), 20290630);
        // A date the game has not set reads 0.
        assert_eq!(Kind::Date.decode(&[2, 0, 0x6C, 0x07]), 0); // 2 Jan 1900
    }

    #[test]
    fn attribute_scale_round_trips() {
        for shown in 1..=20 {
            assert_eq!(Scale::Attribute.shown(Scale::Attribute.stored(shown)), shown);
        }
        assert_eq!(Scale::Attribute.shown(97), 19);
        assert_eq!(Scale::Plain.stored(15), 15);
        for shown in 0..=100 {
            assert_eq!(Scale::Percent.shown(Scale::Percent.stored(shown)), shown);
        }
        assert_eq!(Scale::Percent.shown(8865), 89);
        assert_eq!(Scale::Percent.shown(6950), 70);
    }

    #[test]
    fn flags_keep_the_rest_of_their_byte() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        let players = fields(&layout, Entity::Player);
        let field = |key: &str| players.iter().find(|f| f.key == key).cloned().expect(key);
        let (transfer, loan) = (field("contract.transfer_listed"), field("contract.loan_listed"));
        // Bit 6 is the game's own, set for many youngsters.
        let byte = [0b0100_0010];
        assert_eq!((transfer.decode(&byte), loan.decode(&byte)), (0, 1));
        assert_eq!(transfer.encode(1, &byte), [0b0100_0011]);
        assert_eq!(loan.encode(0, &byte), [0b0100_0000]);
        assert_eq!(transfer.kind.size(), 1);
    }

    #[test]
    fn a_stadium_seats_no_more_than_it_holds() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        let stadium = layout.stadium.as_ref().expect("stadiums are placed");
        let clubs = fields(&layout, Entity::Club);
        let field = |key: &str| clubs.iter().find(|f| f.key == key).cloned().expect(key);
        let limits =
            |key: &str, capacity, seating| stadium_limits(stadium, &field(key), (0, 250_000), capacity, seating);
        // The Emirates holds 60,704, all seated: no more seats, and no less room.
        assert_eq!(limits("stadium.seating", 60_704, 60_704), Some((0, 60_704)));
        assert_eq!(limits("stadium.capacity", 60_704, 60_704), Some((60_704, 250_000)));
        // One that holds 74,879 with 70,000 seated can hold no fewer than 70,000.
        assert_eq!(limits("stadium.capacity", 74_879, 70_000), Some((70_000, 250_000)));
        // Nothing ties how many come, or how far it can grow, to the rest.
        assert_eq!(limits("stadium.used", 60_704, 60_704), None);
        assert_eq!(limits("stadium.expansion", 60_704, 60_704), None);
        // Past the range the editor writes, the range still bounds them.
        assert_eq!(limits("stadium.seating", 300_000, 0), Some((0, 250_000)));
    }

    #[test]
    fn some_fields_take_a_blank_or_a_list_of_codes() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        let players = fields(&layout, Entity::Player);
        let field = |key: &str| players.iter().find(|f| f.key == key).cloned().expect(key);
        let number = field("contract.squad_number");
        assert!(number.takes(1) && number.takes(99) && number.takes(255));
        assert!(!number.takes(0) && !number.takes(100));
        let status = field("contract.squad_status");
        assert!(status.takes(1) && !status.takes(6) && !status.takes(0));
        let price = field("asking_price");
        assert!(price.takes(-1) && price.takes(0) && price.takes(25_000_000) && !price.takes(-2));
        let born = field("date_of_birth");
        assert!(born.takes(19960912) && !born.takes(18991231) && !born.takes(20300101));
        // Reasons to be unhappy are only ever removed, all at once.
        let unhappiness = field("contract.unhappiness");
        assert!(unhappiness.takes(0) && !unhappiness.takes(1));
        // Happiness is a signed byte: Olivier Boscagli's read -4.
        let happiness = field("contract.happiness");
        assert!(happiness.kind == Kind::I8 && happiness.takes(-4) && happiness.takes(100) && !happiness.takes(101));
        // Rested is -500, the least the game keeps.
        let jaded = field("jadedness");
        assert!(jaded.takes(-500) && jaded.takes(252) && !jaded.takes(-501) && jaded.kind == Kind::I16);
        // Sharpness is written from 35%; a player the game created below that can still be edited.
        let sharpness = field("match_sharpness");
        assert!(sharpness.takes(3_500) && !sharpness.takes(3_499) && sharpness.is_plausible(1_200));
        assert!(!sharpness.is_plausible(10_001));
    }

    #[test]
    fn field_tables_cover_the_layout() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        let key = |list: &[Field], key: &str| list.iter().find(|f| f.key == key).cloned().expect(key);
        let players = fields(&layout, Entity::Player);
        // Every position but the sweeper; the contract's wage and end; and the squad number, squad status and the
        // transfer and loan listing, which only players have.
        // The 13 basic fields and the 4 live ones (condition, match sharpness, jadedness, morale); and the day a loan
        // ends where the layout places loans, with its option to buy where it places their terms (Windows so far).
        let loan = usize::from(layout.loan.is_some())
            + usize::from(layout.loan.as_ref().is_some_and(|loan| loan.terms.is_some()));
        assert_eq!(
            players.len(),
            13 + 4 + layout.player.attributes.len() + POSITIONS.len() - 1 + PERSONALITY.len() + 2 + 4 + 4 + 64 + loan
        );
        if layout.loan.is_some() {
            let loan = key(&players, LOAN_EXPIRES);
            assert!(loan.place == Place::Loan && loan.kind == Kind::Date && !loan.mass && !loan.shifted);
        }
        if layout.loan.as_ref().is_some_and(|loan| loan.terms.is_some()) {
            let option = key(&players, LOAN_OPTION_FEE);
            assert!(option.place == Place::Loan && option.kind == Kind::I32 && !option.mass);
            // None is -1, which the editor writes to take an option away.
            assert!(option.takes(-1) && option.takes(10_000_000) && !option.takes(-2));
        }
        assert_eq!(key(&players, "nation").kind, Kind::Pointer);
        assert!(!key(&players, "nation").mass);
        assert!(!key(&players, "contract.squad_number").mass);
        assert!(key(&players, "contract.wage").mass);
        assert!(!players.iter().any(|f| f.key == "position.sw"));
        let staff = fields(&layout, Entity::Staff);
        let staff_attributes = layout.staff.attributes.len() + layout.staff.ratings.len();
        assert_eq!(staff.len(), 6 + staff_attributes + PERSONALITY.len() + 2);
        let clubs = fields(&layout, Entity::Club);
        // Reputation, the 5 facilities and the 4 amounts of money; the 4 capacities of the stadium where the layout
        // knows it (Windows so far), and the share of transfer revenue where it knows that (the Mac so far).
        let stadium = if layout.stadium.is_some() { 4 } else { 0 };
        let transfer_revenue = usize::from(layout.club_finance.transfer_revenue.is_some());
        assert_eq!(clubs.len(), 10 + stadium + transfer_revenue);
        if layout.stadium.is_some() {
            assert_eq!(key(&clubs, "stadium.capacity").place, Place::Stadium);
            // A count of seats, not money, which the app shows with a pound sign.
            assert_eq!(key(&clubs, "stadium.capacity").kind, Kind::U32);
        }

        // Before the person block: never shifted. In it: shifted. Other records: never.
        assert!(!key(&players, "attribute.finishing").shifted);
        assert!(key(&players, "personality.ambition").shifted);
        assert!(key(&players, "international.caps").shifted);
        assert!(!key(&players, "contract.wage").shifted);
        assert!(!key(&staff, "current_ability").shifted);
        assert!(key(&staff, "personality.ambition").shifted);
        assert_eq!(key(&staff, "attribute.authority").scale, Scale::Plain);
        assert_eq!(key(&staff, "attribute.tactical").scale, Scale::Attribute);
        assert_eq!(key(&clubs, "finance.transfer_budget").place, Place::ClubFinance);
        // Only on a system whose layout places it (the Mac's): a percentage in the full finance record.
        if layout.club_finance.transfer_revenue.is_some() {
            let share = key(&clubs, "finance.transfer_revenue");
            assert_eq!((share.place, share.kind, share.min, share.max), (Place::ClubFinance, Kind::U32, 0, 100));
            assert_eq!(share.plausible, 0..=100);
        } else {
            assert!(!clubs.iter().any(|f| f.key == "finance.transfer_revenue"));
        }
        assert_eq!(key(&clubs, "reputation").place, Place::SeniorTeam);

        let target = Target { entity: Entity::Player, uid: 1, address: 0x1000, shift: 0xF8 };
        let finishing = key(&players, "attribute.finishing");
        assert_eq!(finishing.address(target.address, &target), 0x1000 + layout.player.attributes["finishing"]);
        let adaptability = key(&players, "personality.adaptability");
        assert_eq!(adaptability.address(target.address, &target), 0x1000 + layout.player.personality + 0xF8);
        assert_eq!(key(&players, "contract.wage").address(0x9000, &target), 0x9000 + layout.contract.wage);
    }

    #[test]
    fn date_fields_are_plausible_when_unset_or_real() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        let expires = fields(&layout, Entity::Player).into_iter().find(|f| f.key == "contract.expires").expect("field");
        assert!(expires.is_plausible(0));
        assert!(expires.is_plausible(20290630));
        assert!(!expires.is_plausible(20290631));
        assert!(!expires.is_plausible(12345));
    }

    #[test]
    fn list_rows_follow_edits() {
        let mut position_ratings = [1u8; 15];
        position_ratings[12] = 18; // ST
        let mut player = PlayerSummary {
            uid: 1,
            name: "A".to_owned(),
            full_name: None,
            date_of_birth: None,
            age: None,
            nation: None,
            nation_uid: None,
            club: None,
            club_uid: None,
            positions: vec!["st"],
            contract_expires: None,
            current_ability: 120,
            potential_ability: 150,
            position_ratings,
            nation_address: 0,
            attributes: vec![10; crate::ATTRIBUTE_GROUPS.iter().map(|(_, keys)| keys.len()).sum()],
            transfer_listed: false,
            loan_listed: false,
            transfer_value: None,
            woman: None,
            address: 0,
            shift: 0,
        };
        player.note_change("current_ability", 130);
        player.note_change("potential_ability", 160);
        player.note_change("position.dc", 19);
        player.note_change("attribute.finishing", 18);
        player.note_change("contract.transfer_listed", 1);
        player.note_change("contract.expires", 20290630);
        assert_eq!(player.attributes[crate::filter::player_attribute("finishing").unwrap()], 18);
        assert!(player.transfer_listed && !player.loan_listed);
        assert_eq!((player.current_ability, player.potential_ability), (130, 160));
        assert_eq!(player.positions, ["dc", "st"]);
        assert_eq!(player.contract_expires, Some(FmDate { year: 2029, month: 6, day: 30 }));
        player.note_change("position.st", 10);
        assert_eq!(player.positions, ["dc"]);
    }

    #[test]
    fn keys_are_unique() {
        let layout = crate::layout_for(27).expect("FM27 layout");
        for entity in [Entity::Player, Entity::Staff, Entity::Club] {
            let mut keys: Vec<String> = fields(&layout, entity).into_iter().map(|f| f.key).collect();
            let all = keys.len();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), all, "{entity}");
        }
    }
}
