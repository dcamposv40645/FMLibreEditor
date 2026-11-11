//! The overlay: a second window of the app, compact and always on top, that a keyboard shortcut shows over the game and
//! hides again (see `SHORTCUT`). It loads the same interface, which knows it is the overlay by the window's label (see
//! app/src/overlay.ts), and shares the connection, the lists and the change log with the main window, as they live on
//! this side.
//!
//! On macOS a game in fullscreen has a Space of its own, and an app brought to the front takes the screen to a Space of
//! its own: showing the overlay the usual way would leave the game. So there the overlay is a panel that does not
//! activate the app (`NSWindowStyleMaskNonactivatingPanel`), as Spotlight's is: it takes clicks and keys while the game
//! stays the app in front, may join every Space, fullscreen ones too (`NSWindowCollectionBehaviorCanJoinAllSpaces` and
//! `NSWindowCollectionBehaviorFullScreenAuxiliary`), and floats at the level of the status bar's windows. Tauri's
//! window is an `NSWindow` (Tao's `TaoWindow`); it becomes one of our own subclass of `NSPanel`, as the `tauri-nspanel`
//! plugin does it, which says it may be the key window and not the main one, as `TaoWindow` said by its own state. It
//! is shown and hidden by AppKit's own calls, which do not activate the app either.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// The overlay window's label, which the interface asks for.
pub const LABEL: &str = "overlay";

/// What shows or hides the overlay, from any app, until another is chosen: Option-Shift-E on macOS, Alt-Shift-E on
/// Windows. Written as the shortcut plugin reads it: modifiers, then the key's `KeyboardEvent.code`.
pub const DEFAULT_SHORTCUT: &str = "Alt+Shift+KeyE";

/// The file in the app's data folder that keeps the shortcut chosen, as one line.
const SHORTCUT_FILE: &str = "overlay-shortcut.txt";

/// The shortcut that shows the overlay: the one registered now, none when another app holds it, and where it is kept.
pub struct ShortcutSetting {
    file: Option<PathBuf>,
    current: Mutex<Option<String>>,
}

/// Registers the shortcut kept in `data` (the app's data folder), or the default one: another app may hold it, and then
/// the overlay can still be shown from the settings menu, where another shortcut can be chosen.
pub fn setup_shortcut(app: &AppHandle, data: Option<&Path>) {
    let file = data.map(|folder| folder.join(SHORTCUT_FILE));
    let kept = file
        .as_deref()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let wanted = kept.unwrap_or_else(|| DEFAULT_SHORTCUT.to_owned());
    let current = match app.global_shortcut().register(wanted.as_str()) {
        Ok(()) => Some(wanted),
        Err(err) => {
            eprintln!("the overlay's shortcut {wanted} is not available: {err}");
            None
        }
    };
    app.manage(ShortcutSetting { file, current: Mutex::new(current) });
}

/// The shortcut that shows the overlay now; none when another app holds the one chosen.
pub fn shortcut(app: &AppHandle) -> Option<String> {
    let setting = app.state::<ShortcutSetting>();
    setting.current.lock().map_or(None, |current| current.clone())
}

/// Makes `wanted` the shortcut that shows the overlay, and keeps it: it is registered first, so that one another app
/// holds, or one the plugin cannot read, leaves the old one as it was; then the old one is let go.
pub fn set_shortcut(app: &AppHandle, wanted: &str) -> Result<String, String> {
    let setting = app.state::<ShortcutSetting>();
    let mut current = setting.current.lock().map_err(|err| err.to_string())?;
    if current.as_deref() != Some(wanted) {
        let shortcuts = app.global_shortcut();
        shortcuts.register(wanted).map_err(|err| err.to_string())?;
        if let Some(old) = current.take() {
            let _ = shortcuts.unregister(old.as_str());
        }
        *current = Some(wanted.to_owned());
    }
    if let Some(file) = &setting.file {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        std::fs::write(file, wanted).map_err(|err| err.to_string())?;
    }
    Ok(wanted.to_owned())
}

/// The overlay window, made hidden the first time.
fn window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("FMLibreEditor")
        .inner_size(480.0, 780.0)
        .min_inner_size(400.0, 480.0)
        .always_on_top(true)
        .visible(false)
        .build()
}

/// Shows the overlay in front and focused, over the game; hides it when it is in front already.
#[cfg(target_os = "macos")]
pub fn toggle(app: &AppHandle) -> tauri::Result<()> {
    let window = window(app)?;
    // The window is AppKit's, and AppKit is used on the main thread only.
    let address = window.ns_window()? as usize;
    window.run_on_main_thread(move || {
        // SAFETY: `address` is the overlay's NSWindow, which lives as long as the app (it is hidden, never closed by the
        // app), used on the main thread.
        unsafe { panel::toggle(&*(address as *const objc2::runtime::AnyObject)) }
    })
}

/// Hides the overlay, as its own close button does: it is never closed while the app runs (see `before_exit`).
#[cfg(target_os = "macos")]
pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let address = window.ns_window()? as usize;
    window.run_on_main_thread(move || {
        // SAFETY: as in `toggle`.
        unsafe { panel::hide(&*(address as *const objc2::runtime::AnyObject)) }
    })
}

#[cfg(not(target_os = "macos"))]
pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    match app.get_webview_window(LABEL) {
        Some(window) => window.hide(),
        None => Ok(()),
    }
}

/// Before the app quits or restarts, on the main thread: the overlay's window is of Tao's class again, so that AppKit
/// takes it apart as Tao made it. Taken apart as our panel, AppKit threw from `-[NSView removeFromSuperview]`, which
/// ended the app (Steam build 26385295 save, 10 November 2026).
pub fn before_exit(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window(LABEL)
        && let Ok(ns_window) = window.ns_window()
    {
        // SAFETY: the overlay's NSWindow, still alive, used on the main thread (the caller's promise).
        unsafe { panel::restore(&*(ns_window as *const objc2::runtime::AnyObject)) }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(not(target_os = "macos"))]
pub fn toggle(app: &AppHandle) -> tauri::Result<()> {
    let window = window(app)?;
    if window.is_visible()? && window.is_focused()? {
        window.hide()
    } else {
        window.show()?;
        window.set_focus()
    }
}

#[cfg(target_os = "macos")]
mod panel {
    use std::sync::OnceLock;

    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{msg_send, sel};

    /// `NSWindowStyleMaskNonactivatingPanel`.
    const NONACTIVATING: usize = 1 << 7;
    /// `NSWindowCollectionBehaviorCanJoinAllSpaces` and `NSWindowCollectionBehaviorFullScreenAuxiliary`.
    const ALL_SPACES: usize = 1 << 0;
    const FULLSCREEN_AUXILIARY: usize = 1 << 8;
    /// `NSStatusWindowLevel`.
    const STATUS_LEVEL: isize = 25;

    extern "C-unwind" fn yes(_this: &AnyObject, _cmd: Sel) -> Bool {
        Bool::YES
    }

    extern "C-unwind" fn no(_this: &AnyObject, _cmd: Sel) -> Bool {
        Bool::NO
    }

    /// The window's own class (Tao's), from before it became a panel: it is that again before the app quits.
    static ORIGINAL: OnceLock<usize> = OnceLock::new();

    /// Our subclass of `NSPanel`: it may become the key window, to take keys, and never the main one.
    fn class() -> &'static AnyClass {
        static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
        CLASS.get_or_init(|| {
            let panel = AnyClass::get(c"NSPanel").expect("AppKit's NSPanel");
            let mut builder = ClassBuilder::new(c"FMLibreOverlayPanel", panel).expect("a class of that name is ours");
            // SAFETY: both are `- (BOOL)` methods of NSWindow, given functions of that signature.
            unsafe {
                builder.add_method(sel!(canBecomeKeyWindow), yes as extern "C-unwind" fn(_, _) -> _);
                builder.add_method(sel!(canBecomeMainWindow), no as extern "C-unwind" fn(_, _) -> _);
            }
            builder.register()
        })
    }

    /// Makes `window` the overlay's panel, once: of our class, not activating the app, on every Space and over
    /// fullscreen ones, floating.
    ///
    /// # Safety
    /// `window` is a Tao `NSWindow`, used on the main thread.
    unsafe fn make_panel(window: &AnyObject) {
        let class = class();
        if std::ptr::eq(window.class(), class) {
            return;
        }
        // SAFETY: NSPanel adds no state of its own to NSWindow, and the window keeps its memory: it only answers as a
        // panel now (see the module's notes).
        ORIGINAL.get_or_init(|| std::ptr::from_ref(window.class()) as usize);
        unsafe {
            objc2::ffi::object_setClass(std::ptr::from_ref(window).cast_mut(), class);
            let mask: usize = msg_send![window, styleMask];
            let _: () = msg_send![window, setStyleMask: mask | NONACTIVATING];
            let _: () = msg_send![window, setFloatingPanel: true];
            let _: () = msg_send![window, setHidesOnDeactivate: false];
            let behavior: usize = msg_send![window, collectionBehavior];
            let _: () = msg_send![window, setCollectionBehavior: behavior | ALL_SPACES | FULLSCREEN_AUXILIARY];
            let _: () = msg_send![window, setLevel: STATUS_LEVEL];
        }
    }

    /// Hides the panel.
    ///
    /// # Safety
    /// As [`make_panel`].
    pub(super) unsafe fn hide(window: &AnyObject) {
        // SAFETY: as the caller promises.
        unsafe {
            let _: () = msg_send![window, orderOut: None::<&AnyObject>];
        }
    }

    /// Gives the window its own class back (see `before_exit`), when it is our panel.
    ///
    /// # Safety
    /// As [`make_panel`].
    pub(super) unsafe fn restore(window: &AnyObject) {
        let Some(&original) = ORIGINAL.get() else {
            return;
        };
        if std::ptr::eq(window.class(), class()) {
            // SAFETY: the class the window had, whose state it still holds.
            unsafe {
                let _: () = msg_send![window, orderOut: None::<&AnyObject>];
                objc2::ffi::object_setClass(std::ptr::from_ref(window).cast_mut(), original as *const AnyClass);
            }
        }
    }

    /// Shows the panel in front and makes it the key window, without activating the app; hides it when it is both.
    ///
    /// # Safety
    /// As [`make_panel`].
    pub(super) unsafe fn toggle(window: &AnyObject) {
        // SAFETY: as the caller promises.
        unsafe {
            make_panel(window);
            let visible: bool = msg_send![window, isVisible];
            let key: bool = msg_send![window, isKeyWindow];
            if visible && key {
                let _: () = msg_send![window, orderOut: None::<&AnyObject>];
            } else {
                let _: () = msg_send![window, orderFrontRegardless];
                let _: () = msg_send![window, makeKeyWindow];
            }
        }
    }
}
