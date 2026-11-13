//! What the game's screen shows: the person, team or club whose screen is open in FM, read from the game's interface,
//! so that the editor can follow it.
//!
//! The interface is Unity code built with IL2CPP, which keeps the names of its classes and of their fields. One object
//! of the class `GameWorldTabsAndBookmarksModule` runs the tabs of the screen. Its field
//! `m_currentSecondaryTabObjectUniqueID` holds the UID of what the screen shows, and `m_lastTabUpdateValue` a typed
//! value whose `m_value` is a reference to it, whose class says what it is: `FM.UI.PersonReference` for a player or a
//! member of staff, `FM.UI.TeamReference` for a team, whose UID is the team's own (see `TeamLayout::uid`). See
//! docs/recon/fm27-viewing.md.
//!
//! The class, its fields and the object are found by their names, once (`ScreenWatch::find`, a few seconds: three
//! passes over the game's memory); reading what the screen shows then takes a handful of reads (`ScreenWatch::read`).

use std::collections::HashMap;

use fmlibre_mem::{Process, Region, is_user_pointer, regions};

use crate::{ClubSummary, Session};

/// The class whose one object runs the tabs of the screen.
const MODULE_CLASS: &str = "GameWorldTabsAndBookmarksModule";
/// Its field with the UID of what the screen shows.
const ID_FIELD: &str = "m_currentSecondaryTabObjectUniqueID";
/// Its field with a typed value that refers to what the screen shows.
const VALUE_FIELD: &str = "m_lastTabUpdateValue";
/// Its field with the panel open on the screen.
const PANEL_FIELD: &str = "m_currentOpenPanel";
/// The field of a typed value (`SI.Core.TypedValue<T>`) with the value it holds.
const TYPED_VALUE_FIELD: &str = "m_value";

/// Where IL2CPP keeps, in a class (`Il2CppClass`), its name, its namespace, its parent class and its fields; and how a
/// field (`FieldInfo`) gives its name and its offset in the object. As in the Unity of FM27 on macOS.
const CLASS_NAME: u64 = 0x10;
const CLASS_NAMESPACE: u64 = 0x18;
const CLASS_PARENT: u64 = 0x58;
const CLASS_FIELDS: u64 = 0x80;
const FIELD_SIZE: u64 = 0x20;
const FIELD_OFFSET: u64 = 0x18;
/// More fields than any class here has: a list read past its end stops at the first entry that is not a field.
const MOST_FIELDS: u64 = 400;
/// How many classes up from a typed value's own to look for `m_value`.
const MOST_PARENTS: usize = 6;

const CHUNK: u64 = 8 << 20;

/// What the screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewed {
    /// A player or a member of staff, by UID.
    Person(u32),
    /// A team, by its own UID: a club's first team has the club's UID, its other teams others.
    Team(u32),
    /// A club, by UID.
    Club(u32),
    /// Something else: a competition, the inbox, a screen of no one.
    Other,
}

/// Why `ScreenWatch::read` read nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchError {
    /// The object is gone (the game closed, or rebuilt its interface): `ScreenWatch::find` looks for it again.
    Gone,
}

/// The object that runs the screen's tabs, and where its fields are.
#[derive(Debug)]
pub struct ScreenWatch {
    module: u64,
    class: u64,
    id_at: u64,
    value_at: u64,
    /// The offset of `m_value` in each class of typed value seen, and what each class of reference means.
    value_fields: HashMap<u64, Option<u64>>,
    kinds: HashMap<u64, Kind>,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Person,
    Team,
    Club,
    Other,
}

impl ScreenWatch {
    /// Looks for the object that runs the screen's tabs. None when the game has no such class or object (yet: the
    /// interface is built after the game starts).
    pub fn find(process: &Process) -> Option<Self> {
        let names = find_text(process, MODULE_CLASS);
        let class = find_words(process, &names)
            .into_iter()
            .map(|at| at - CLASS_NAME)
            .find(|&class| class_name(process, class).as_deref() == Some(MODULE_CLASS))?;
        let fields = class_fields(process, class);
        let id_at = *fields.get(ID_FIELD)?;
        let value_at = *fields.get(VALUE_FIELD)?;
        // The class's objects start with a pointer to it, as do the game's tables of its methods and others. The one
        // that runs the screen has a panel open and a typed value: objects of classes of their own.
        let panel_at = *fields.get(PANEL_FIELD)?;
        let module = find_words(process, &[class]).into_iter().find(|&object| {
            is_object(process, process.read_u64(object + panel_at).unwrap_or(0))
                && is_object(process, process.read_u64(object + value_at).unwrap_or(0))
        })?;
        Some(Self { module, class, id_at, value_at, value_fields: HashMap::new(), kinds: HashMap::new() })
    }

    /// What the screen shows now.
    pub fn read(&mut self, process: &Process) -> Result<Viewed, WatchError> {
        if process.read_u64(self.module).map_err(|_| WatchError::Gone)? != self.class {
            return Err(WatchError::Gone);
        }
        let uid = process.read_u32(self.module + self.id_at).map_err(|_| WatchError::Gone)?;
        let kind = self.value_kind(process).unwrap_or(Kind::Other);
        Ok(match kind {
            Kind::Person => Viewed::Person(uid),
            Kind::Team => Viewed::Team(uid),
            Kind::Club => Viewed::Club(uid),
            Kind::Other => Viewed::Other,
        })
    }

    /// What the reference in `m_lastTabUpdateValue` refers to, by its class.
    fn value_kind(&mut self, process: &Process) -> Option<Kind> {
        let value = process.read_u64(self.module + self.value_at).ok().filter(|&v| is_user_pointer(v))?;
        let value_class = process.read_u64(value).ok()?;
        let field = *self.value_fields.entry(value_class).or_insert_with(|| typed_value_field(process, value_class));
        let reference = process.read_u64(value + field?).ok().filter(|&r| is_user_pointer(r))?;
        let class = process.read_u64(reference).ok()?;
        Some(*self.kinds.entry(class).or_insert_with(|| kind_of(class_name(process, class).as_deref())))
    }
}

/// Whether `at` is an object of the interface: it starts with a pointer to a class with a name.
fn is_object(process: &Process, at: u64) -> bool {
    is_user_pointer(at)
        && process.read_u64(at).is_ok_and(|class| is_user_pointer(class) && class_name(process, class).is_some())
}

/// Where a team is: its club's UID and its place in the club's list of teams (0 is the first team).
pub type TeamPlace = (u32, usize);

impl Session {
    /// Every team of `clubs`, by its own UID (see `TeamLayout::uid`): empty where the layout has not placed it. One
    /// read of each club's list of teams and of each team's UID.
    pub fn team_places<'a>(&self, clubs: impl IntoIterator<Item = &'a ClubSummary>) -> HashMap<u32, TeamPlace> {
        let mut places = HashMap::new();
        let Some(uid_at) = self.layout.team.uid else {
            return places;
        };
        let teams_at = self.layout.club.teams;
        for club in clubs {
            let (Ok(begin), Ok(end)) =
                (self.process.read_u64(club.address + teams_at), self.process.read_u64(club.address + teams_at + 8))
            else {
                continue;
            };
            // A club has a handful of teams.
            if !is_user_pointer(begin) || end < begin || end - begin > 0x100 {
                continue;
            }
            let Ok(list) = self.process.read_bytes(begin, (end - begin) as usize) else {
                continue;
            };
            for (index, word) in list.as_chunks::<8>().0.iter().enumerate() {
                let team = u64::from_le_bytes(*word);
                if let Some(uid) = is_user_pointer(team).then(|| self.process.read_u32(team + uid_at).ok()).flatten() {
                    places.entry(uid).or_insert((club.uid, index));
                }
            }
        }
        places
    }
}

/// What a class of reference refers to, by its name.
fn kind_of(name: Option<&str>) -> Kind {
    let short = name.and_then(|name| name.rsplit('.').next()).unwrap_or("");
    match short {
        "PersonReference" | "PlayerReference" | "StaffReference" | "NonPlayerReference" => Kind::Person,
        "TeamReference" => Kind::Team,
        "ClubReference" => Kind::Club,
        _ => Kind::Other,
    }
}

/// The offset of `m_value` in the class of typed value `class` or one of its parents.
fn typed_value_field(process: &Process, class: u64) -> Option<u64> {
    let mut class = class;
    for _ in 0..MOST_PARENTS {
        if let Some(&at) = class_fields(process, class).get(TYPED_VALUE_FIELD) {
            return Some(at);
        }
        class = process.read_u64(class + CLASS_PARENT).ok().filter(|&c| is_user_pointer(c))?;
    }
    None
}

/// A class's name with its namespace, e.g. `FM.UI.TeamReference`; none when `class` is not one.
fn class_name(process: &Process, class: u64) -> Option<String> {
    let name = c_string(process, process.read_u64(class + CLASS_NAME).ok()?)?;
    let namespace = c_string(process, process.read_u64(class + CLASS_NAMESPACE).ok()?)?;
    if !is_name(&name) || !(namespace.is_empty() || namespace.split('.').all(is_name)) {
        return None;
    }
    Some(if namespace.is_empty() { name } else { format!("{namespace}.{name}") })
}

/// A class's own fields (not its parents'), by name: their offsets in an object. Static fields are among them too.
fn class_fields(process: &Process, class: u64) -> HashMap<String, u64> {
    let mut fields = HashMap::new();
    let Some(list) = process.read_u64(class + CLASS_FIELDS).ok().filter(|&l| is_user_pointer(l)) else {
        return fields;
    };
    for i in 0..MOST_FIELDS {
        let entry = list + i * FIELD_SIZE;
        let Some(name) = process.read_u64(entry).ok().and_then(|at| c_string(process, at)).filter(|n| is_field(n))
        else {
            break;
        };
        let Ok(offset) = process.read_u32(entry + FIELD_OFFSET) else {
            break;
        };
        // The first of two fields of the same name is the class's own.
        fields.entry(name).or_insert(u64::from(offset));
    }
    fields
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || "_`<>[],+".contains(c))
}

fn is_field(text: &str) -> bool {
    is_name(text.strip_prefix('<').unwrap_or(text))
}

/// The text at `at`, up to its nul; none when it is not readable, is longer than a name, or is not ASCII. Names are
/// packed one after the other, so `at` need not be aligned.
fn c_string(process: &Process, at: u64) -> Option<String> {
    if !(0x1_0000..0x8000_0000_0000).contains(&at) {
        return None;
    }
    let bytes = process.read_bytes(at, 0x80).ok()?;
    let end = bytes.iter().position(|&b| b == 0)?;
    let text = std::str::from_utf8(&bytes[..end]).ok()?;
    text.is_ascii().then(|| text.to_owned())
}

/// Every place in the game's readable memory that holds `text` as a whole nul-ended string.
fn find_text(process: &Process, text: &str) -> Vec<u64> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(text.as_bytes());
    needle.push(0);
    let finder = memchr::memmem::Finder::new(&needle);
    let mut found = Vec::new();
    for region in regions(process, 0x1_0000, 0x7FFF_FFFF_0000).into_iter().filter(Region::is_readable) {
        let mut offset = 0;
        while offset < region.size {
            let base = region.base + offset;
            // A little more than a chunk, so that a string across the boundary is seen.
            let len = (region.size - offset).min(CHUNK + needle.len() as u64);
            if let Ok(bytes) = process.read_bytes(base, len as usize) {
                found.extend(finder.find_iter(&bytes).map(|i| base + i as u64 + 1));
            }
            offset += CHUNK;
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// Every 8-byte-aligned place in the game's own data (not in the threads' stacks) that holds one of `values`.
fn find_words(process: &Process, values: &[u64]) -> Vec<u64> {
    let mut found = Vec::new();
    if values.is_empty() {
        return found;
    }
    for region in regions(process, 0x1_0000, 0x7FFF_FFFF_0000).into_iter().filter(|r| r.holds_data() && !is_stack(r)) {
        let mut offset = 0;
        while offset < region.size {
            let base = region.base + offset;
            let len = (region.size - offset).min(CHUNK);
            if let Ok(bytes) = process.read_bytes(base, len as usize) {
                for (i, word) in bytes.as_chunks::<8>().0.iter().enumerate() {
                    if values.contains(&u64::from_le_bytes(*word)) {
                        found.push(base + (i * 8) as u64);
                    }
                }
            }
            offset += CHUNK;
        }
    }
    found
}

#[cfg(target_os = "macos")]
fn is_stack(region: &Region) -> bool {
    const VM_MEMORY_STACK: u32 = 30;
    region.user_tag == VM_MEMORY_STACK
}

#[cfg(not(target_os = "macos"))]
fn is_stack(_region: &Region) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_told_apart_by_their_class() {
        assert!(matches!(kind_of(Some("FM.UI.PersonReference")), Kind::Person));
        assert!(matches!(kind_of(Some("FM.UI.TeamReference")), Kind::Team));
        assert!(matches!(kind_of(Some("FM.UI.ClubReference")), Kind::Club));
        assert!(matches!(kind_of(Some("SI.Bindable.NullReference")), Kind::Other));
        assert!(matches!(kind_of(None), Kind::Other));
    }

    #[test]
    fn names_are_those_of_classes_and_fields() {
        assert!(is_name("GameWorldTabsAndBookmarksModule"));
        assert!(is_name("Dictionary`2"));
        assert!(!is_name("2abc"));
        assert!(!is_name(""));
        assert!(is_field("m_currentSecondaryTabObjectUniqueID"));
        assert!(is_field("<Type>k__BackingField"));
        assert!(!is_field("has space"));
    }
}
