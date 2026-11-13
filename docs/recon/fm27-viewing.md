# FM27: what the game's screen shows

Notes behind the setting "Follow the game's screen" (`ScreenWatch` in `crates/fmlibre-core/src/viewing.rs`, the
thread in `app/src-tauri/src/follow.rs`): the editor opens the profile of the player, member of staff or club whose
screen FM shows, or the squad of the team it shows. Found on the Mac Steam build 26385295 (Apple silicon), on a
Barcelona save of 2 May 2034, on 12 November 2026. Nothing here writes to the game.

## Where it is

FM's screens are Unity code built with IL2CPP (`GameAssembly.dylib`), not `game_plugin`, which has no class for
them. IL2CPP keeps the names of the classes and of their fields, so everything below is found by name:

| What | Where |
| --- | --- |
| The object that runs the screen's tabs | the one object of the class `GameWorldTabsAndBookmarksModule` (no namespace) whose `m_currentOpenPanel` and `m_lastTabUpdateValue` point at objects |
| The UID of what the screen shows | its field `m_currentSecondaryTabObjectUniqueID` (`+0x2A8` on that build), a `u32` |
| What kind of thing that is | its field `m_lastTabUpdateValue` (`+0x360`): an `SI.Core.ReferenceTypedValue`, whose `m_value` (`+0x18`, a field of its parent `SI.Core.TypedValue<T>`) is a reference whose class says: `FM.UI.PersonReference`, `FM.UI.TeamReference` or `FM.UI.ClubReference` |
| A team's own UID | `+0x0C` of the team (`uid` in `[team]` of `layout-macos.toml`) |

The field also holds a club's or a team's UID. A first team's UID is its club's (Barcelona's first team: 1708). The
club's other teams have UIDs of their own: Barcelona's under-19s, 2000779733. The game's `TeamReference` names the team
by its database index (`m_index`, `+0x1C`, which is `+0x08` of the team) and a type (`<Type>k__BackingField`, `+0x20`,
24 for a team). The editor uses the UID rather than these.

The IL2CPP structures it reads are:

- a class (`Il2CppClass`): its name at `+0x10`, its namespace at `+0x18`, its parent at `+0x58` and its fields at
  `+0x80`;
- a field (`FieldInfo`, `0x20` bytes): its name at `+0x00` and its offset in the object at `+0x18`.

## Finding it

`fmlibre-probe view-find <what FM shows> <file> --against <earlier files>` saves every pointer to what the screen shows,
and every 4-byte word that holds its UID, in the game's writable memory. It then prints the places that held what each
screen showed.

1. **Players' pointers, on Dinho's, Chris Chalk's and Álvaro Cortés's profiles.** Ten places held the player each
   time. They were all on one thread's stack, which is no place to read from.
2. **Players' UIDs, on the same three profiles.** 35 places held them. Of these:
   - 34 were in objects of the class `SI.Bindable.PanelSubtabInfo`, at `+0xA8`: one for each tab of the profile;
   - one was at `+0x2A8` of the `GameWorldTabsAndBookmarksModule`.
3. **The module's class.** Its field list names `+0x2A8` `m_currentSecondaryTabObjectUniqueID`, beside
   `m_currentViewedEntity` (`+0x240`), `m_currentViewedEntityType` (`+0x260`) and `m_lastTabUpdateValue` (`+0x360`).
4. **Barcelona's first-team squad screen.** `+0x2A8` held 1708, and `m_lastTabUpdateValue` a `TeamReference`.
   `m_currentViewedEntity` held a `NullReference` there, so the editor reads the kind from `m_lastTabUpdateValue`.
5. **The under-19s' squad screen.** It showed 2000779733, the UID at `+0x0C` of Barcelona's second team.

## Confirming it

`fmlibre-probe viewing --watch` found the module in about 5 s and followed the screen as it changed. It read:

- `team 2000779733: Barcelona team 1`;
- `team 1708: Barcelona team 0`;
- `player 2002089220 Dinho`;
- `staff 4201107 Joan Barbarà`;
- `club 679 Manchester City`.

Between screens it read, for a moment, what is not there: `person 1708` (the new kind, the old UID) and
`club 4294967295`. The app takes what the screen shows only once it reads the same twice in a row, 0.4 s apart, and
only when its lists have it.

## Open

- **Windows.** IL2CPP's structures, and the team's `+0x0C`, are likely the same there, but nothing has been read on
  Windows yet. To check it on a save, `fmlibre-probe viewing --watch` should follow the screen. Then add `uid = 0x0C` to
  `[team]` in `layout.toml`. Without it, the editor follows players, staff and clubs but not teams.
- **Other screens.** A competition, a nation or the inbox reads as "something else", which the editor ignores. Which of
  their references are worth following has not been looked at.
- **After an update of the game.** The names hold across builds unless SI renames them. The offsets in the IL2CPP
  structures hold unless Unity changes its version.
