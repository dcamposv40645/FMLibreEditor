//! Players or clubs written to a file that a spreadsheet or a script reads: CSV, a row each, or JSON, an object each.
//! What a profile holds that is worth comparing, across players or clubs and over time. For a player: abilities,
//! positions, the attributes on the 1–20 scale the game shows, personality, reputation, caps, and the contract with its
//! clauses. For a club: reputation, supporters, facilities, the stadium and the finances. Each with the in-game date
//! they were read on. Money is in pounds, as the game keeps it. Only reads memory.

use std::io::{self, Write};

use serde::{Deserialize, Serialize, Serializer};

use crate::{
    ATTRIBUTE_GROUPS, ClubDetails, ClubSummary, Contract, FmDate, International, PERSONALITY, PlayerDetails,
    PlayerSummary, RELEASE_CLAUSE, RELEASE_CLAUSES, Reputation, SUPPORTER_MIX, SUPPORTER_TRAITS, Session,
    read_club_details, read_player_details, release_clause_expiry, release_clause_never_expires,
};

/// How an export writes the players or clubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// Values separated by commas, a row per player or club.
    Csv,
    /// The same, separated by semicolons: what Excel takes where the comma is the decimal mark (in Turkish, Spanish
    /// or German, say).
    CsvSemicolon,
    /// An object per player or club, with groups of values (a player's attributes, a club's facilities) as objects of
    /// their own.
    Json,
}

impl ExportFormat {
    /// The extension of the file's name.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv | ExportFormat::CsvSemicolon => "csv",
            ExportFormat::Json => "json",
        }
    }
}

/// What an export says about where it comes from: at the top of a JSON file. A CSV file has the in-game date on every
/// row, so that exports can be put together and compared.
#[derive(Debug, Clone)]
pub struct ExportHeader {
    /// What wrote it, e.g. "FMLibreEditor 0.4.1".
    pub writer: String,
    /// The game it was read from, e.g. "FM27 Steam build 26340721".
    pub game: String,
    pub game_date: Option<FmDate>,
}

/// What an export wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Exported {
    /// The players or clubs written.
    pub count: usize,
    /// Players or clubs whose objects had changed since the lists were read: left out.
    pub skipped: usize,
}

/// The position ratings an export has, in the order of the pitch: each but the sweeper, which the game no longer plays.
const RATINGS: [&str; 14] = ["gk", "dl", "dc", "dr", "wbl", "wbr", "dm", "ml", "mc", "mr", "aml", "amc", "amr", "st"];

impl Session {
    /// Writes `players`, in their order, to `out` as `format`. Each is read again, as his profile is; one whose object
    /// has changed since the lists were read is left out and counted. Only reads memory.
    pub fn export_players(
        &self,
        players: &[&PlayerSummary],
        format: ExportFormat,
        header: &ExportHeader,
        out: &mut impl Write,
    ) -> io::Result<Exported> {
        let mut exported = Exported::default();
        let details =
            players.iter().filter_map(|player| match read_player_details(&self.process, &self.layout, player) {
                Ok(Some(details)) => {
                    exported.count += 1;
                    Some(details)
                }
                _ => {
                    exported.skipped += 1;
                    None
                }
            });
        match format {
            ExportFormat::Csv => write_csv(out, header, details, b',')?,
            ExportFormat::CsvSemicolon => write_csv(out, header, details, b';')?,
            ExportFormat::Json => write_json(out, header, details)?,
        }
        Ok(exported)
    }

    /// Writes `clubs`, in their order, to `out` as `format`. Each is read again, as its profile is; one whose object
    /// has changed since the lists were read is left out and counted. Only reads memory.
    pub fn export_clubs(
        &self,
        clubs: &[&ClubSummary],
        format: ExportFormat,
        header: &ExportHeader,
        out: &mut impl Write,
    ) -> io::Result<Exported> {
        let mut exported = Exported::default();
        let details = clubs.iter().filter_map(|club| match read_club_details(&self.process, &self.layout, club) {
            Ok(Some(details)) => {
                exported.count += 1;
                Some(details)
            }
            _ => {
                exported.skipped += 1;
                None
            }
        });
        match format {
            ExportFormat::Csv => write_club_csv(out, header, details, b',')?,
            ExportFormat::CsvSemicolon => write_club_csv(out, header, details, b';')?,
            ExportFormat::Json => write_club_json(out, header, details)?,
        }
        Ok(exported)
    }
}

/// A cell of a CSV row, read from a player's or a club's profile.
type Cell<T> = Box<dyn Fn(&T) -> String>;

/// The columns of a CSV export, in order: the name of each, and how it reads a profile.
struct Columns<T>(Vec<(String, Cell<T>)>);

impl<T> Default for Columns<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Columns<T> {
    fn add(&mut self, name: impl Into<String>, cell: impl Fn(&T) -> String + 'static) {
        self.0.push((name.into(), Box::new(cell)));
    }
}

impl Columns<PlayerDetails> {
    /// A column read from the contract: empty for a player without one.
    fn contract(&mut self, name: &str, cell: fn(&Contract) -> String) {
        self.add(name, move |d| d.contract.as_ref().map(cell).unwrap_or_default());
    }
}

fn csv_columns(game_date: Option<FmDate>) -> Columns<PlayerDetails> {
    let mut columns = Columns::<PlayerDetails>::default();
    columns.add("game_date", move |_| game_date.map(iso).unwrap_or_default());
    columns.add("uid", |d| d.summary.uid.to_string());
    columns.add("name", |d| d.summary.name.clone());
    columns.add("full_name", |d| d.summary.full_name.clone().unwrap_or_default());
    columns.add("date_of_birth", |d| d.summary.date_of_birth.map(iso).unwrap_or_default());
    columns.add("age", |d| text(d.summary.age));
    columns.add("nationality", |d| d.summary.nation.clone().unwrap_or_default());
    columns.add("club", |d| d.summary.club.clone().unwrap_or_default());
    columns.add("positions", |d| positions(&d.summary).join(" "));
    columns.add("current_ability", |d| d.summary.current_ability.to_string());
    columns.add("potential_ability", |d| d.summary.potential_ability.to_string());
    columns.add("height_cm", |d| d.height_cm.to_string());
    for key in RATINGS {
        columns.add(format!("rating_{key}"), move |d| text(d.position_ratings.get(key)));
    }
    for key in ATTRIBUTE_GROUPS.iter().flat_map(|(_, keys)| keys.iter().copied()) {
        columns.add(key, move |d| text(d.attributes.get(key).map(|attribute| attribute.value)));
    }
    for key in PERSONALITY {
        columns.add(key, move |d| text(d.personality.get(key)));
    }
    columns.add("reputation_home", |d| d.reputation.home.to_string());
    columns.add("reputation_current", |d| d.reputation.current.to_string());
    columns.add("reputation_world", |d| d.reputation.world.to_string());
    columns.add("international_caps", |d| d.international.caps.to_string());
    columns.add("international_goals", |d| d.international.goals.to_string());
    columns.add("u21_caps", |d| d.international.u21_caps.to_string());
    columns.add("u21_goals", |d| d.international.u21_goals.to_string());
    columns.add("asking_price_gbp", |d| text(d.asking_price));
    columns.contract("wage_per_week_gbp", |c| c.wage.to_string());
    columns.contract("contract_signed", |c| c.signed.map(iso).unwrap_or_default());
    columns.contract("contract_expires", |c| c.expires.map(iso).unwrap_or_default());
    columns.contract("contract_joined", |c| c.joined.map(iso).unwrap_or_default());
    columns.contract("contract_type", |c| {
        c.contract_type
            .map(|code| contract_type_name(code).map_or_else(|| code.to_string(), str::to_owned))
            .unwrap_or_default()
    });
    columns.contract("squad_status", |c| {
        squad_status_name(c.squad_status).map_or_else(|| c.squad_status.to_string(), str::to_owned)
    });
    columns.contract("squad_number", |c| text(c.squad_number));
    columns.contract("transfer_listed", |c| c.transfer_listed.to_string());
    columns.contract("loan_listed", |c| c.loan_listed.to_string());
    columns.contract("release_clause_gbp", |c| text(release_clause(c)));
    columns.contract("clauses", clauses_text);
    columns
}

/// The facilities a club export has, in the order of the club profile; corporate only where the club has the full
/// finance record.
const FACILITIES: [&str; 5] = ["training", "youth_facilities", "youth_coaching", "youth_recruitment", "corporate"];

/// The columns of a club export. `decimal_mark` is what the ticket price, the one value with a fraction, is written
/// with: a comma beside a semicolon separator, for the Excel that takes the comma as the decimal mark.
fn club_columns(game_date: Option<FmDate>, decimal_mark: char) -> Columns<ClubDetails> {
    let mut columns = Columns::<ClubDetails>::default();
    columns.add("game_date", move |_| game_date.map(iso).unwrap_or_default());
    columns.add("uid", |d| d.summary.uid.to_string());
    columns.add("name", |d| d.summary.name.clone());
    columns.add("short_name", |d| d.summary.short_name.clone().unwrap_or_default());
    columns.add("women", |d| d.summary.women.to_string());
    columns.add("reputation", |d| text(d.summary.reputation));
    columns.add("founded", |d| text(d.summary.founded));
    columns.add("season_tickets", |d| text(d.season_tickets));
    columns.add("social_media_followers", |d| text(d.social_media));
    for key in SUPPORTER_MIX {
        columns.add(format!("supporters_{key}_pct"), move |d| text(d.supporter_mix.get(key)));
    }
    for key in SUPPORTER_TRAITS {
        columns.add(format!("supporter_{key}"), move |d| text(d.supporter_traits.get(key)));
    }
    for (key, name) in [
        ("training", "training_facilities"),
        ("youth_facilities", "youth_facilities"),
        ("youth_coaching", "youth_coaching"),
        ("youth_recruitment", "youth_recruitment"),
        ("corporate", "corporate_facilities"),
    ] {
        columns.add(name, move |d| text(d.facilities.get(key)));
    }
    columns.add("stadium", |d| d.stadium.as_ref().and_then(|s| s.name.clone()).unwrap_or_default());
    columns.add("stadium_capacity", |d| text(d.stadium.as_ref().map(|s| s.capacity)));
    columns.add("stadium_seating", |d| text(d.stadium.as_ref().map(|s| s.seating)));
    columns.add("stadium_capacity_used", |d| text(d.stadium.as_ref().map(|s| s.used)));
    columns.add("stadium_expansion", |d| text(d.stadium.as_ref().map(|s| s.expansion)));
    columns.add("balance_gbp", |d| text(d.finances.map(|f| f.balance)));
    columns.add("transfer_budget_gbp", |d| text(d.finances.map(|f| f.transfer_budget)));
    columns.add("wage_spend_per_week_gbp", |d| text(d.finances.map(|f| f.wage_budget)));
    columns.add("wage_budget_per_week_gbp", |d| text(d.finances.map(|f| f.total_wage_budget)));
    columns.add("ticket_price_gbp", move |d| {
        let price = d.finances.map(|f| format!("{:.2}", f.ticket_price));
        price.map_or_else(String::new, |price| price.replace('.', &decimal_mark.to_string()))
    });
    columns
}

/// Writes the players as CSV: a row of column names, then a row per player, each cell apart by `separator`.
fn write_csv(
    out: &mut impl Write,
    header: &ExportHeader,
    players: impl Iterator<Item = PlayerDetails>,
    separator: u8,
) -> io::Result<()> {
    write_rows(out, &csv_columns(header.game_date), players, separator)
}

/// Writes the clubs as CSV: a row of column names, then a row per club, each cell apart by `separator`.
fn write_club_csv(
    out: &mut impl Write,
    header: &ExportHeader,
    clubs: impl Iterator<Item = ClubDetails>,
    separator: u8,
) -> io::Result<()> {
    let decimal_mark = if separator == b';' { ',' } else { '.' };
    write_rows(out, &club_columns(header.game_date, decimal_mark), clubs, separator)
}

/// Writes a CSV file: a row of the names of `columns`, then a row per profile, each cell apart by `separator`.
fn write_rows<T>(
    out: &mut impl Write,
    columns: &Columns<T>,
    rows: impl Iterator<Item = T>,
    separator: u8,
) -> io::Result<()> {
    // The byte order mark tells Excel the file is UTF-8, so that the accents in names come through.
    out.write_all("\u{FEFF}".as_bytes())?;
    write_csv_row(out, columns.0.iter().map(|(name, _)| name.as_str()), separator)?;
    for row in rows {
        let cells: Vec<String> = columns.0.iter().map(|(_, cell)| cell(&row)).collect();
        write_csv_row(out, cells.iter().map(String::as_str), separator)?;
    }
    Ok(())
}

/// One line of a CSV file: the cells apart by `separator`, each in quotes where it holds the separator, a quote or a
/// line break, with its quotes doubled.
fn write_csv_row<'a>(out: &mut impl Write, cells: impl Iterator<Item = &'a str>, separator: u8) -> io::Result<()> {
    for (i, cell) in cells.enumerate() {
        if i > 0 {
            out.write_all(&[separator])?;
        }
        if cell.bytes().any(|byte| byte == separator || matches!(byte, b'"' | b'\n' | b'\r')) {
            write!(out, "\"{}\"", cell.replace('"', "\"\""))?;
        } else {
            out.write_all(cell.as_bytes())?;
        }
    }
    out.write_all(b"\r\n")
}

/// Writes the players as JSON: where they come from, then the players, one object to a line.
fn write_json(
    out: &mut impl Write,
    header: &ExportHeader,
    players: impl Iterator<Item = PlayerDetails>,
) -> io::Result<()> {
    write_json_header(out, header)?;
    writeln!(out, "  \"attributeScale\": \"1-20\",")?;
    write!(out, "  \"players\": [")?;
    for (i, details) in players.enumerate() {
        out.write_all(if i == 0 { b"\n    " } else { b",\n    " })?;
        serde_json::to_writer(&mut *out, &JsonPlayer::from(&details))?;
    }
    writeln!(out, "\n  ]\n}}")
}

/// Writes the clubs as JSON: where they come from, then the clubs, one object to a line.
fn write_club_json(
    out: &mut impl Write,
    header: &ExportHeader,
    clubs: impl Iterator<Item = ClubDetails>,
) -> io::Result<()> {
    write_json_header(out, header)?;
    write!(out, "  \"clubs\": [")?;
    for (i, details) in clubs.enumerate() {
        out.write_all(if i == 0 { b"\n    " } else { b",\n    " })?;
        serde_json::to_writer(&mut *out, &JsonClub::from(&details))?;
    }
    writeln!(out, "\n  ]\n}}")
}

/// The start of a JSON export: where it comes from, and the currency of its money.
fn write_json_header(out: &mut impl Write, header: &ExportHeader) -> io::Result<()> {
    writeln!(out, "{{")?;
    writeln!(out, "  \"writer\": {},", serde_json::to_string(&header.writer)?)?;
    writeln!(out, "  \"game\": {},", serde_json::to_string(&header.game)?)?;
    writeln!(out, "  \"gameDate\": {},", serde_json::to_string(&header.game_date.map(iso))?)?;
    writeln!(out, "  \"currency\": \"GBP\",")
}

/// A player as a JSON export writes him.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonPlayer<'a> {
    uid: u32,
    name: &'a str,
    full_name: Option<&'a str>,
    date_of_birth: Option<String>,
    age: Option<u16>,
    nationality: Option<&'a str>,
    club: Option<&'a str>,
    positions: Vec<String>,
    current_ability: u16,
    potential_ability: i16,
    height_cm: u16,
    position_ratings: InOrder<String, Option<u8>>,
    attributes: InOrder<&'static str, u8>,
    personality: InOrder<&'static str, Option<u8>>,
    reputation: Reputation,
    international: International,
    /// Pounds; none when the club has set none.
    asking_price: Option<i32>,
    contract: Option<JsonContract>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonContract {
    /// Pounds.
    wage_per_week: i32,
    signed: Option<String>,
    expires: Option<String>,
    joined: Option<String>,
    squad_status: u8,
    squad_status_name: Option<&'static str>,
    /// The game's code for the kind of contract; none where the build's layout does not know where it is.
    contract_type: Option<u8>,
    contract_type_name: Option<&'static str>,
    squad_number: Option<u8>,
    transfer_listed: bool,
    loan_listed: bool,
    clauses: Vec<JsonClause>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonClause {
    /// The game's code for what it is.
    kind: u16,
    name: Option<&'static str>,
    /// Pounds.
    amount: Option<u32>,
    /// Goals, assists, league games, years or a percentage, by kind; for a release clause, the days from the
    /// contract's start until it expires.
    condition: Option<u16>,
    /// The day a release clause expires.
    expires: Option<String>,
}

impl<'a> From<&'a PlayerDetails> for JsonPlayer<'a> {
    fn from(d: &'a PlayerDetails) -> Self {
        let p = &d.summary;
        let attributes = ATTRIBUTE_GROUPS.iter().flat_map(|(_, keys)| keys.iter().copied());
        Self {
            uid: p.uid,
            name: &p.name,
            full_name: p.full_name.as_deref(),
            date_of_birth: p.date_of_birth.map(iso),
            age: p.age,
            nationality: p.nation.as_deref(),
            club: p.club.as_deref(),
            positions: positions(p),
            current_ability: p.current_ability,
            potential_ability: p.potential_ability,
            height_cm: d.height_cm,
            position_ratings: InOrder(
                RATINGS.iter().map(|&key| (key.to_uppercase(), d.position_ratings.get(key).copied())).collect(),
            ),
            attributes: InOrder(attributes.filter_map(|key| Some((key, d.attributes.get(key)?.value))).collect()),
            personality: InOrder(PERSONALITY.iter().map(|&key| (key, d.personality.get(key).copied())).collect()),
            reputation: d.reputation,
            international: d.international,
            asking_price: d.asking_price,
            contract: d.contract.as_ref().map(|c| JsonContract {
                wage_per_week: c.wage,
                signed: c.signed.map(iso),
                expires: c.expires.map(iso),
                joined: c.joined.map(iso),
                squad_status: c.squad_status,
                squad_status_name: squad_status_name(c.squad_status),
                contract_type: c.contract_type,
                contract_type_name: c.contract_type.and_then(contract_type_name),
                squad_number: c.squad_number,
                transfer_listed: c.transfer_listed,
                loan_listed: c.loan_listed,
                clauses: c
                    .clauses
                    .iter()
                    .map(|clause| JsonClause {
                        kind: clause.kind,
                        name: clause_name(clause.kind),
                        amount: clause.amount,
                        condition: clause.condition,
                        expires: release_clause_expiry(clause, c.signed).map(iso),
                    })
                    .collect(),
            }),
        }
    }
}

/// A club as a JSON export writes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonClub<'a> {
    uid: u32,
    name: &'a str,
    short_name: Option<&'a str>,
    women: bool,
    /// 0–10000, of the senior team.
    reputation: Option<u16>,
    founded: Option<u16>,
    season_tickets: Option<u32>,
    social_media_followers: Option<u32>,
    /// Percentages, by kind of supporter; empty when not known.
    supporter_mix: InOrder<&'static str, u8>,
    /// 1–20; empty when not known.
    supporter_traits: InOrder<&'static str, u8>,
    /// 1–20; corporate only with the full finance record.
    facilities: InOrder<&'static str, u8>,
    stadium: Option<JsonStadium<'a>>,
    /// None for a club without the full finance record: FM keeps finances only for clubs in the leagues being played.
    finances: Option<JsonFinances>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonStadium<'a> {
    name: Option<&'a str>,
    capacity: u32,
    seating: u32,
    /// The capacity in use when it is limited; 0 when it is not.
    capacity_used: u32,
    /// The most it can be expanded to.
    expansion: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonFinances {
    /// Pounds.
    balance: i32,
    /// Pounds.
    transfer_budget: i32,
    /// What the club pays in wages now, pounds a week (the game's "Spending Wage"). Read from the
    /// finance record's `wage_budget`.
    wage_spend_per_week: i32,
    /// The wage limit, pounds a week; minus the spend it is the game's available wage budget. Read
    /// from the finance record's `total_wage_budget`.
    wage_budget_per_week: i32,
    /// The average match ticket, pounds.
    ticket_price: f32,
}

impl<'a> From<&'a ClubDetails> for JsonClub<'a> {
    fn from(d: &'a ClubDetails) -> Self {
        let c = &d.summary;
        let in_order = |keys: &[&'static str], values: &std::collections::BTreeMap<&'static str, u8>| {
            InOrder(keys.iter().filter_map(|&key| Some((key, *values.get(key)?))).collect())
        };
        Self {
            uid: c.uid,
            name: &c.name,
            short_name: c.short_name.as_deref(),
            women: c.women,
            reputation: c.reputation,
            founded: c.founded,
            season_tickets: d.season_tickets,
            social_media_followers: d.social_media,
            supporter_mix: in_order(&SUPPORTER_MIX, &d.supporter_mix),
            supporter_traits: in_order(&SUPPORTER_TRAITS, &d.supporter_traits),
            facilities: in_order(&FACILITIES, &d.facilities),
            stadium: d.stadium.as_ref().map(|s| JsonStadium {
                name: s.name.as_deref(),
                capacity: s.capacity,
                seating: s.seating,
                capacity_used: s.used,
                expansion: s.expansion,
            }),
            finances: d.finances.map(|f| JsonFinances {
                balance: f.balance,
                transfer_budget: f.transfer_budget,
                wage_spend_per_week: f.wage_budget,
                wage_budget_per_week: f.total_wage_budget,
                ticket_price: pence(f.ticket_price),
            }),
        }
    }
}

/// Keys and values written as one JSON object, in their order.
struct InOrder<K, V>(Vec<(K, V)>);

impl<K: Serialize, V: Serialize> Serialize for InOrder<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(key, value)| (key, value)))
    }
}

/// "2027-01-02".
fn iso(date: FmDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

/// Pounds rounded to the penny.
fn pence(pounds: f32) -> f32 {
    (pounds * 100.0).round() / 100.0
}

/// A value as text; nothing for none.
fn text<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

/// The positions he plays (rated 15 or more), best first, as the game abbreviates them: "AMR", "AML".
fn positions(player: &PlayerSummary) -> Vec<String> {
    player.positions.iter().map(|key| key.to_uppercase()).collect()
}

/// The amount of the contract's minimum fee release clause, if it has one.
fn release_clause(contract: &Contract) -> Option<u32> {
    contract.clauses.iter().find(|clause| clause.kind == RELEASE_CLAUSE).and_then(|clause| clause.amount)
}

/// The clauses and bonuses of a contract in words, apart by semicolons: "Minimum fee release clause: 100000000 GBP;
/// Yearly wage rise: 10%".
fn clauses_text(contract: &Contract) -> String {
    contract.clauses.iter().map(|clause| clause_text(clause, contract.signed)).collect::<Vec<_>>().join("; ")
}

/// A clause or bonus in words, with what it pays and depends on: "Seasonal landmark goal bonus: 250000 GBP, 25 goals",
/// "Minimum fee release clause: 50000000 GBP, expires 2029-09-01"; `signed`, the contract's start, dates a release
/// clause's expiry.
fn clause_text(clause: &crate::Clause, signed: Option<FmDate>) -> String {
    let name = clause_name(clause.kind).map_or_else(|| format!("Clause {}", clause.kind), str::to_owned);
    let condition = match (release_clause_expiry(clause, signed), clause.condition) {
        (Some(expires), _) => Some(format!("expires {}", iso(expires))),
        // A mark, not a day (see `MOST_RELEASE_CLAUSE_DAYS`).
        (None, Some(_)) if release_clause_never_expires(clause) => None,
        (None, Some(value)) => Some(condition_text(clause.kind, value)),
        (None, None) => None,
    };
    let amount = clause.amount.map(|amount| match clause.kind {
        // The wage he gets from then on.
        13 | 19 => format!("{amount} GBP a week"),
        _ => format!("{amount} GBP"),
    });
    match (amount, condition) {
        // The percentage of the profit over the amount.
        (Some(amount), Some(condition)) if clause.kind == SELL_ON_PROFIT => {
            format!("{name}: {condition} of the profit over {amount}")
        }
        (Some(amount), Some(condition)) => format!("{name}: {amount}, {condition}"),
        (Some(amount), None) => format!("{name}: {amount}"),
        (None, Some(condition)) => format!("{name}: {condition}"),
        (None, None) => name,
    }
}

/// The kind of the sell-on clause whose amount is a threshold: the club that sold the player gets the percentage of
/// the profit over it.
const SELL_ON_PROFIT: u16 = 9;

/// The English name of a squad status, by the code a contract holds it as; as the app's English dictionary names it
/// (see docs/recon/fm27-contracts.md).
fn squad_status_name(code: u8) -> Option<&'static str> {
    Some(match code {
        1 => "Star player",
        2 => "Important player",
        3 => "Regular starter",
        4 => "Squad player",
        5 => "Impact sub",
        7 => "Fringe player",
        9 => "Emergency backup",
        10 => "Breakthrough prospect",
        11 => "Future prospect",
        13 => "Youngster",
        14 => "B team regular",
        15 => "First-choice goalkeeper",
        16 => "Cup goalkeeper",
        17 => "Domestic cup goalkeeper",
        20 => "Backup",
        21 => "Emergency backup (goalkeeper)",
        22 => "Surplus to requirements",
        _ => return None,
    })
}

/// The English name of a kind of contract, by the code a contract holds it as; as the app's English dictionary names
/// it.
fn contract_type_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0 => "Part-time contract",
        1 => "Full-time contract",
        2 => "Amateur contract",
        3 => "Youth contract",
        4 => "Non-contract",
        10 => "Senior minimum salary contract",
        11 => "Senior contract",
        _ => return None,
    })
}

/// The English name of a clause or bonus, by its kind; as the app's English dictionary names it (see
/// docs/recon/fm27-contracts.md).
fn clause_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        0 => "Minimum fee release clause",
        1 => "Relegation release clause",
        2 => "Non promotion release clause",
        3 => "Yearly wage rise",
        4 => "Promotion wage rise",
        5 => "Relegation wage drop",
        6 => "Release clause for a staff role",
        8 => "Sell on fee percentage",
        9 => "Sell on fee profit percentage",
        10 => "Seasonal landmark goal bonus",
        11 => "One-year extension after league games (final season)",
        12 => "Match highest earner clause",
        13 => "Wage after reaching club league games",
        14 => "Top division promotion wage rise",
        15 => "Top division relegation wage drop",
        16 => "Minimum fee release clause (foreign clubs)",
        17 => "Minimum fee release clause (domestic clubs in higher division)",
        18 => "Minimum fee release clause (domestic clubs)",
        19 => "Wage after reaching international appearances",
        20 => "Percentage of wages paid by sponsor",
        21 => "Percentage of gate receipts paid to player",
        22 => "Optional contract extension by club",
        23 => "Waive club compensation for managerial role",
        25 => "One-year extension after league games (promoted final season)",
        26 => "One-year extension after league games (avoid relegation final season)",
        27 => "Minimum fee release clause (clubs in UEFA Champions League)",
        28 => "MLS guaranteed contract clause",
        29 => "Contract extension after promotion",
        30 => "Injury release clause",
        31 => "Minimum fee release clause (clubs in a continental competition)",
        32 => "Appearance fee",
        33 => "Goal bonus",
        34 => "Clean sheet bonus",
        37 => "International cap bonus",
        38 => "Unused substitute fee",
        39 => "Assist bonus",
        40 => "Seasonal landmark assist bonus",
        41 => "Seasonal landmark combined goal and assist bonus",
        54 => "Not shown in the game (54)",
        55 => "Active relegation release clause",
        56 => "Active non promotion release clause",
        58 => "Not shown in the game (58)",
        63 => "Not shown in the game (63)",
        _ => return None,
    })
}

/// What a clause depends on, in words, by its kind: goals, assists, league games, international matches, years or a
/// percentage; for a release clause whose contract's start is not known, the days after it.
fn condition_text(kind: u16, value: u16) -> String {
    let plural = |one: &str, many: &str| if value == 1 { format!("1 {one}") } else { format!("{value} {many}") };
    match kind {
        10 => plural("goal", "goals"),
        40 => plural("assist", "assists"),
        41 => format!("{value} goals and assists"),
        19 => plural("international appearance", "international appearances"),
        11 | 13 | 25 | 26 => plural("league game", "league games"),
        22 | 29 => plural("year", "years"),
        3 | 4 | 5 | 8 | 9 | 14 | 15 | 20 | 21 => format!("{value}%"),
        _ if RELEASE_CLAUSES.contains(&kind) => format!("expires {} after signing", plural("day", "days")),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::clubs::Stadium;
    use crate::{Attribute, Clause, Finances, POSITIONS};

    fn saka() -> PlayerDetails {
        let summary = PlayerSummary {
            uid: 28122642,
            name: "Bukayo Saka".to_owned(),
            full_name: Some("Bukayo Ayoyinka Saka".to_owned()),
            date_of_birth: Some(FmDate { year: 2001, month: 9, day: 5 }),
            age: Some(25),
            nation: Some("England".to_owned()),
            nation_uid: Some(765),
            nation_address: 0,
            club: Some("Arsenal".to_owned()),
            club_uid: Some(602),
            positions: vec!["amr", "aml"],
            contract_expires: Some(FmDate { year: 2031, month: 6, day: 30 }),
            current_ability: 181,
            potential_ability: 188,
            position_ratings: [1; 15],
            attributes: Vec::new(),
            transfer_listed: false,
            loan_listed: false,
            transfer_value: None,
            woman: Some(false),
            address: 0,
            shift: 0,
        };
        let attributes: BTreeMap<String, Attribute> = ATTRIBUTE_GROUPS
            .iter()
            .flat_map(|(_, keys)| keys.iter())
            .map(|&key| (key.to_owned(), Attribute::from_raw(70)))
            .collect();
        PlayerDetails {
            summary,
            height_cm: 178,
            asking_price: None,
            reputation: Reputation { home: 8000, current: 7600, world: 7400 },
            international: International { caps: 50, goals: 12, u21_caps: 10, u21_goals: 3 },
            position_ratings: POSITIONS.iter().map(|&key| (key, if key == "amr" { 20 } else { 10 })).collect(),
            personality: PERSONALITY.iter().map(|&key| (key, 15)).collect(),
            attributes,
            contract: Some(Contract {
                wage: 195_000,
                signed: Some(FmDate { year: 2025, month: 5, day: 23 }),
                expires: Some(FmDate { year: 2031, month: 6, day: 30 }),
                joined: Some(FmDate { year: 2018, month: 7, day: 1 }),
                squad_status: 1,
                contract_type: Some(1),
                squad_number: Some(7),
                transfer_listed: false,
                loan_listed: false,
                loyalty_bonus: 0,
                happiness: 0,
                playing_time_happiness: 0,
                clauses: vec![
                    Clause { index: 0, kind: 0, amount: Some(150_000_000), condition: None },
                    Clause { index: 1, kind: 3, amount: None, condition: Some(10) },
                ],
                concerns: Vec::new(),
            }),
            condition: 10_000,
            match_sharpness: 10_000,
            jadedness: -500,
            morale: 15,
            traits: Vec::new(),
            injuries: Vec::new(),
            bans: Vec::new(),
            ties: Vec::new(),
            tie_room: 0,
            ties_grow: false,
            retirement_known: false,
            retires: None,
            clause_room: 0,
            loan: None,
        }
    }

    /// A club with everything a profile can hold, and one outside the leagues being played: no finances, no stadium.
    fn ajax() -> ClubDetails {
        ClubDetails {
            summary: ClubSummary {
                uid: 1013,
                name: "AFC Ajax".to_owned(),
                short_name: Some("Ajax".to_owned()),
                women: false,
                reputation: Some(7_200),
                founded: Some(1900),
                address: 0,
            },
            season_tickets: Some(42_000),
            social_media: Some(9_500_000),
            supporter_mix: SUPPORTER_MIX.iter().zip([10, 30, 25, 15, 15, 5]).map(|(&key, pct)| (key, pct)).collect(),
            supporter_traits: SUPPORTER_TRAITS.iter().map(|&key| (key, 14)).collect(),
            facilities: FACILITIES.iter().map(|&key| (key, 17)).collect(),
            finances: Some(Finances {
                balance: 61_250_000,
                transfer_budget: 35_180_000,
                wage_budget: 975_000,
                total_wage_budget: 1_060_000,
                ticket_price: 31.456,
                transfer_revenue: Some(60),
            }),
            stadium: Some(Stadium {
                name: Some("Johan Cruijff ArenA".to_owned()),
                capacity: 55_865,
                seating: 55_865,
                used: 0,
                expansion: 60_000,
            }),
        }
    }

    fn amateurs() -> ClubDetails {
        let mut club = ajax();
        club.summary.uid = 99;
        club.summary.name = "Amateurs, Zaterdag".to_owned();
        club.summary.short_name = None;
        club.summary.reputation = None;
        club.facilities.remove("corporate");
        club.finances = None;
        club.stadium = None;
        club
    }

    fn header() -> ExportHeader {
        ExportHeader {
            writer: "FMLibreEditor test".to_owned(),
            game: "FM27 Steam build 26340721".to_owned(),
            game_date: Some(FmDate { year: 2027, month: 1, day: 2 }),
        }
    }

    #[test]
    fn csv_has_a_header_and_a_row_of_the_same_length() {
        let mut out = Vec::new();
        write_csv(&mut out, &header(), std::iter::once(saka()), b',').unwrap();
        let text = String::from_utf8(out).unwrap();
        let text = text.strip_prefix('\u{FEFF}').expect("a byte order mark");
        let lines: Vec<&str> = text.split("\r\n").filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 2);
        let header: Vec<&str> = lines[0].split(',').collect();
        assert_eq!(&header[..4], ["game_date", "uid", "name", "full_name"]);
        assert!(header.contains(&"finishing") && header.contains(&"rating_amr") && header.contains(&"ambition"));
        assert!(!header.contains(&"rating_sw"));
        // The clauses hold no comma, so the row splits like the header.
        let row: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(row.len(), header.len());
        let cell = |name: &str| row[header.iter().position(|&column| column == name).unwrap()];
        assert_eq!(cell("game_date"), "2027-01-02");
        assert_eq!(cell("positions"), "AMR AML");
        assert_eq!(cell("rating_amr"), "20");
        assert_eq!(cell("finishing"), "14");
        assert_eq!(cell("squad_status"), "Star player");
        assert_eq!(cell("contract_type"), "Full-time contract");
        assert_eq!(cell("release_clause_gbp"), "150000000");
        assert_eq!(cell("asking_price_gbp"), "");
        assert_eq!(cell("clauses"), "Minimum fee release clause: 150000000 GBP; Yearly wage rise: 10%");
    }

    #[test]
    fn clauses_in_words() {
        let clause = |kind, amount, condition| Clause { index: 0, kind, amount, condition };
        let signed = Some(FmDate { year: 2027, month: 7, day: 3 });
        let said = |kind, amount, condition| clause_text(&clause(kind, amount, condition), signed);
        // Ben Schlicke's release clause for domestic clubs in a higher division: 791 days after 3 July 2027.
        assert_eq!(
            said(17, Some(20_000_000), Some(791)),
            "Minimum fee release clause (domestic clubs in higher division): 20000000 GBP, expires 2029-09-01"
        );
        assert_eq!(
            clause_text(&clause(17, Some(20_000_000), Some(791)), None),
            "Minimum fee release clause (domestic clubs in higher division): 20000000 GBP, expires 791 days after signing"
        );
        assert_eq!(said(27, Some(5), None), "Minimum fee release clause (clubs in UEFA Champions League): 5 GBP");
        // Sylla's sell-on clause: 7% of the profit over £2.1M.
        assert_eq!(
            said(9, Some(2_147_579), Some(7)),
            "Sell on fee profit percentage: 7% of the profit over 2147579 GBP"
        );
        assert_eq!(
            said(13, Some(118_462), Some(30)),
            "Wage after reaching club league games: 118462 GBP a week, 30 league games"
        );
        assert_eq!(said(10, Some(250_000), Some(1)), "Seasonal landmark goal bonus: 250000 GBP, 1 goal");
        assert_eq!(said(14, None, Some(20)), "Top division promotion wage rise: 20%");
        assert_eq!(said(56, Some(161_288), None), "Active non promotion release clause: 161288 GBP");
        assert_eq!(
            said(19, Some(158_689), Some(5)),
            "Wage after reaching international appearances: 158689 GBP a week, 5 international appearances"
        );
        // Jon Moncayola's 65,505 is a mark, not the day 2202.
        assert_eq!(said(0, Some(1_000_000), Some(65_505)), "Minimum fee release clause: 1000000 GBP");
        assert_eq!(said(63, None, None), "Not shown in the game (63)");
        assert_eq!(said(99, None, Some(3)), "Clause 99: 3");
    }

    #[test]
    fn every_squad_status_and_contract_type_seen_is_named() {
        for code in [1, 2, 3, 4, 5, 7, 9, 10, 11, 13, 14, 15, 16, 17, 20, 21, 22] {
            assert!(squad_status_name(code).is_some(), "{code}");
        }
        assert_eq!(squad_status_name(4), Some("Squad player"));
        for code in [0, 1, 2, 3, 4, 10, 11] {
            assert!(contract_type_name(code).is_some(), "{code}");
        }
    }

    #[test]
    fn csv_quotes_what_holds_the_separator() {
        let mut out = Vec::new();
        write_csv_row(&mut out, ["plain", "a;b", "say \"hi\"", "a,b"].into_iter(), b';').unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "plain;\"a;b\";\"say \"\"hi\"\"\";a,b\r\n");
    }

    #[test]
    fn json_reads_back_in_order() {
        let mut out = Vec::new();
        write_json(&mut out, &header(), [saka(), saka()].into_iter()).unwrap();
        let text = String::from_utf8(out).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["gameDate"], "2027-01-02");
        assert_eq!(value["players"].as_array().unwrap().len(), 2);
        let player = &value["players"][0];
        assert_eq!(player["positions"], serde_json::json!(["AMR", "AML"]));
        assert_eq!(player["attributes"]["finishing"], 14);
        assert_eq!(player["positionRatings"]["AMR"], 20);
        assert_eq!(player["contract"]["squadStatusName"], "Star player");
        assert_eq!(player["contract"]["contractType"], 1);
        assert_eq!(player["contract"]["contractTypeName"], "Full-time contract");
        assert_eq!(player["contract"]["clauses"][0]["amount"], 150_000_000);
        assert_eq!(player["contract"]["clauses"][0]["expires"], serde_json::Value::Null);
        // The object keeps the order it was written in: the UID first, the attributes in the game's groups.
        let line = text.lines().find(|line| line.contains("28122642")).unwrap();
        assert!(line.trim_start().starts_with("{\"uid\":28122642,\"name\":\"Bukayo Saka\""));
        assert!(line.find("\"corners\"").unwrap() < line.find("\"finishing\"").unwrap());
    }

    #[test]
    fn club_csv_has_a_header_and_a_row_of_the_same_length() {
        let mut out = Vec::new();
        write_club_csv(&mut out, &header(), [ajax(), amateurs()].into_iter(), b';').unwrap();
        let text = String::from_utf8(out).unwrap();
        let text = text.strip_prefix('\u{FEFF}').expect("a byte order mark");
        let lines: Vec<&str> = text.split("\r\n").filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 3);
        let header: Vec<&str> = lines[0].split(';').collect();
        assert_eq!(&header[..4], ["game_date", "uid", "name", "short_name"]);
        let row: Vec<&str> = lines[1].split(';').collect();
        assert_eq!(row.len(), header.len());
        let cell = |row: &[&str], name: &str| row[header.iter().position(|&column| column == name).unwrap()].to_owned();
        assert_eq!(cell(&row, "game_date"), "2027-01-02");
        assert_eq!(cell(&row, "short_name"), "Ajax");
        assert_eq!(cell(&row, "women"), "false");
        assert_eq!(cell(&row, "supporters_hardcore_pct"), "10");
        assert_eq!(cell(&row, "supporter_loyalty"), "14");
        assert_eq!(cell(&row, "training_facilities"), "17");
        assert_eq!(cell(&row, "corporate_facilities"), "17");
        assert_eq!(cell(&row, "stadium"), "Johan Cruijff ArenA");
        assert_eq!(cell(&row, "transfer_budget_gbp"), "35180000");
        assert_eq!(cell(&row, "wage_spend_per_week_gbp"), "975000");
        assert_eq!(cell(&row, "wage_budget_per_week_gbp"), "1060000");
        assert_eq!(cell(&row, "ticket_price_gbp"), "31,46");
        // Without finances or a stadium, their cells are empty, and the row is as long as the header.
        let row: Vec<&str> = lines[2].split(';').collect();
        assert_eq!(row.len(), header.len());
        assert_eq!(cell(&row, "balance_gbp"), "");
        assert_eq!(cell(&row, "corporate_facilities"), "");
        assert_eq!(cell(&row, "stadium_capacity"), "");
        assert_eq!(cell(&row, "reputation"), "");
    }

    #[test]
    fn club_csv_quotes_a_name_with_a_comma() {
        let mut out = Vec::new();
        write_club_csv(&mut out, &header(), std::iter::once(amateurs()), b',').unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(",\"Amateurs, Zaterdag\","));
    }

    #[test]
    fn club_csv_writes_the_ticket_price_to_the_penny_with_a_point_beside_commas() {
        let mut out = Vec::new();
        write_club_csv(&mut out, &header(), std::iter::once(ajax()), b',').unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.trim_end().ends_with(",31.46"));
    }

    #[test]
    fn club_json_reads_back_in_order() {
        let mut out = Vec::new();
        write_club_json(&mut out, &header(), [ajax(), amateurs()].into_iter()).unwrap();
        let text = String::from_utf8(out).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["gameDate"], "2027-01-02");
        assert_eq!(value["currency"], "GBP");
        assert!(value.get("attributeScale").is_none());
        assert_eq!(value["clubs"].as_array().unwrap().len(), 2);
        let club = &value["clubs"][0];
        assert_eq!(club["shortName"], "Ajax");
        assert_eq!(club["socialMediaFollowers"], 9_500_000);
        assert_eq!(club["supporterMix"]["fair_weather"], 15);
        assert_eq!(club["supporterMix"]["corporate"], 15);
        assert_eq!(club["supporterMix"]["casual"], 5);
        assert_eq!(club["facilities"]["corporate"], 17);
        assert_eq!(club["stadium"]["capacityUsed"], 0);
        assert_eq!(club["finances"]["transferBudget"], 35_180_000);
        assert_eq!(club["finances"]["wageSpendPerWeek"], 975_000);
        assert_eq!(club["finances"]["wageBudgetPerWeek"], 1_060_000);
        assert_eq!(club["finances"]["ticketPrice"], 31.46);
        let amateurs = &value["clubs"][1];
        assert_eq!(amateurs["finances"], serde_json::Value::Null);
        assert_eq!(amateurs["stadium"], serde_json::Value::Null);
        assert!(amateurs["facilities"].get("corporate").is_none());
        // The object keeps the order it was written in: the UID first, the supporters in the order the game keeps them.
        let line = text.lines().find(|line| line.contains("\"uid\":1013")).unwrap();
        assert!(line.trim_start().starts_with("{\"uid\":1013,\"name\":\"AFC Ajax\""));
        assert!(line.find("\"hardcore\"").unwrap() < line.find("\"casual\"").unwrap());
        let facilities = line.find("\"facilities\"").unwrap();
        assert!(line[facilities..].find("\"training\"").unwrap() < line[facilities..].find("\"corporate\"").unwrap());
    }
}
