// The main window's zoom, kept between runs (see app/src/zoom.ts). The page keeps the level in its storage and sets the
// webview's zoom itself, but on Windows the webview sets it back to 100% as the page finishes loading, after the page
// has set it at startup. So the page also tells this side the level (`remember_zoom`), which keeps it in a file of the
// app's data, and this side sets it once the main window's page has loaded (`on_page_load`).

use std::path::PathBuf;

use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Runtime, Webview};

/// The window zoomed: the main one (the overlay keeps its own size).
const MAIN: &str = "main";

/// The file in the app's data that holds the level, as "1.5".
const FILE: &str = "zoom.txt";

/// The levels the page offers go from 0.8 to 2: anything far outside is no level of its.
const LEVELS: std::ops::RangeInclusive<f64> = 0.5..=3.0;

fn file<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|folder| folder.join(FILE))
}

/// Keeps `level`, the zoom the main window is drawn at, for the next run.
#[tauri::command]
pub fn remember_zoom(app: AppHandle, level: f64) {
    if !LEVELS.contains(&level) {
        return;
    }
    if let Some(file) = file(&app) {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(file, level.to_string());
    }
}

/// Once the main window's page has loaded, draws it at the level kept.
pub fn on_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    if payload.event() != PageLoadEvent::Finished || webview.label() != MAIN {
        return;
    }
    let level = file(webview.app_handle())
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|level| LEVELS.contains(level));
    let Some(level) = level else {
        return;
    };
    let _ = webview.set_zoom(level);
    // The webview can set its zoom back to 100% a little after the page has loaded too, as the window first shows:
    // the level is drawn again over the first seconds.
    let webview = webview.clone();
    std::thread::spawn(move || {
        for wait in AGAIN_AFTER {
            std::thread::sleep(wait);
            let _ = webview.set_zoom(level);
        }
    });
}

/// When the level is drawn again after the page has loaded, each after the one before.
const AGAIN_AFTER: [std::time::Duration; 4] = [
    std::time::Duration::from_millis(250),
    std::time::Duration::from_millis(750),
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(2),
];
