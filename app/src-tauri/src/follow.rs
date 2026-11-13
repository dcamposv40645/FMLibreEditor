// Following FM's screen: while it is on, a thread looks at what the game's screen shows every little while (see
// `ScreenWatch` in fmlibre-core's viewing.rs) and, when that changes to a player, a member of staff, a team or a club
// the lists have, tells the windows: `game-viewing`, with its row, on which they open its profile (see followView.ts).
//
// Finding where the screen keeps it takes a few seconds, once per game; the thread does it with a process of its own,
// without holding the connection, so the app goes on as it is meanwhile. What the screen shows is taken once it reads
// the same twice in a row: while FM changes screens, it gives the new screen's UID a moment before its kind.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fmlibre_core::{ClubSummary, PlayerSummary, ScreenWatch, StaffSummary, TeamPlace, Viewed};
use fmlibre_mem::Process;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, Connection};

/// How often the screen is looked at.
const EVERY: Duration = Duration::from_millis(400);

/// How long after not finding the screen's tabs to look for them again: the game may still be loading.
const RETRY: Duration = Duration::from_secs(10);

/// How long after a team was not found among the clubs' teams to read them all again.
const TEAMS_AGAIN: Duration = Duration::from_secs(30);

/// Whether the windows want to follow the screen (see `set_follow_view`).
#[derive(Default)]
pub struct Follow(Arc<AtomicBool>);

/// What the game's screen shows, as the windows get it: the row its profile opens with.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Viewing {
    Player {
        player: PlayerSummary,
    },
    Staff {
        staff: StaffSummary,
    },
    /// A team of `club`, by its place in the club's list of teams: 0 is the first team.
    Team {
        club: ClubSummary,
        team: usize,
    },
    Club {
        club: ClubSummary,
    },
}

/// Turns following the game's screen on or off. Turned on, the windows are told what it shows now.
#[tauri::command]
pub fn set_follow_view(follow: State<'_, Follow>, on: bool) {
    follow.0.store(on, Ordering::Relaxed);
}

/// The screen's tabs in the game with the process id `pid`.
struct Watching {
    pid: u32,
    process: Process,
    screen: ScreenWatch,
}

/// Starts the thread that follows the game's screen.
pub fn start(handle: AppHandle) {
    let follow = handle.state::<Follow>().0.clone();
    std::thread::spawn(move || {
        let mut watching: Option<Watching> = None;
        // When the screen's tabs were last looked for in vain, in the game with that process id.
        let mut missed: Option<(u32, Instant)> = None;
        let mut seen: Option<Viewed> = None;
        let mut told: Option<Viewed> = None;
        let mut teams: HashMap<u32, TeamPlace> = HashMap::new();
        let mut teams_read: Option<Instant> = None;
        loop {
            std::thread::sleep(EVERY);
            if !follow.load(Ordering::Relaxed) {
                // Turned on again, the windows are told what the screen shows then.
                told = None;
                continue;
            }
            let state = handle.state::<AppState>();
            let pid = state.0.lock().ok().and_then(|guard| guard.as_ref().map(|c| c.session.process.pid()));
            let Some(pid) = pid else {
                (watching, seen, told) = (None, None, None);
                continue;
            };
            if watching.as_ref().is_some_and(|w| w.pid != pid) {
                (watching, seen, told) = (None, None, None);
                teams.clear();
                teams_read = None;
            }
            if watching.is_none() {
                if missed.is_some_and(|(at, when)| at == pid && when.elapsed() < RETRY) {
                    continue;
                }
                let found = Process::open_read(pid)
                    .ok()
                    .and_then(|process| ScreenWatch::find(&process).map(|screen| Watching { pid, process, screen }));
                missed = found.is_none().then(|| (pid, Instant::now()));
                watching = found;
                continue;
            }
            let Some(watch) = watching.as_mut() else {
                continue;
            };
            let Ok(viewed) = watch.screen.read(&watch.process) else {
                // Gone: looked for again.
                (watching, seen) = (None, None);
                continue;
            };
            if seen != Some(viewed) {
                seen = Some(viewed);
                continue;
            }
            if told == Some(viewed) {
                continue;
            }
            told = Some(viewed);
            let viewing = {
                let Ok(guard) = state.0.lock() else {
                    continue;
                };
                let Some(connection) = guard.as_ref() else {
                    continue;
                };
                if let Viewed::Team(uid) = viewed
                    && !teams.contains_key(&uid)
                    && teams_read.is_none_or(|at| at.elapsed() >= TEAMS_AGAIN)
                {
                    teams = connection.session.team_places(connection.clubs.iter());
                    teams_read = Some(Instant::now());
                }
                viewing_of(connection, &teams, viewed)
            };
            if let Some(viewing) = viewing {
                let _ = handle.emit("game-viewing", viewing);
            }
        }
    });
}

/// The row of what the screen shows, when the lists have it.
fn viewing_of(connection: &Connection, teams: &HashMap<u32, TeamPlace>, viewed: Viewed) -> Option<Viewing> {
    match viewed {
        Viewed::Person(uid) => connection
            .players
            .get(uid)
            .map(|player| Viewing::Player { player: player.clone() })
            .or_else(|| connection.staff.get(uid).map(|staff| Viewing::Staff { staff: staff.clone() })),
        Viewed::Team(uid) => {
            let &(club, team) = teams.get(&uid)?;
            connection.clubs.get(club).map(|club| Viewing::Team { club: club.clone(), team })
        }
        Viewed::Club(uid) => connection.clubs.get(uid).map(|club| Viewing::Club { club: club.clone() }),
        Viewed::Other => None,
    }
}
