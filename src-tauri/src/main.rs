// No console window behind the launcher in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod idle;
mod timing;

use launcher_core::search::{sort_items, Engine, Hit};
use launcher_core::settings::Settings;
use launcher_core::theme::{self, Theme, DEFAULT_THEME};
use launcher_core::usage::Usage;
use launcher_core::{apps, now_secs, Action, Item};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use timing::{HotkeyTimings, Timing};

const WINDOW: &str = "main";
/// Rows returned per search. The list never needs virtualizing at this size.
const RESULT_LIMIT: usize = 50;
/// Showing a window can briefly report a focus loss; ignore those.
const BLUR_GRACE: Duration = Duration::from_millis(250);
const IDLE_SETTLE: Duration = Duration::from_secs(3);
const IDLE_WINDOW: Duration = Duration::from_secs(30);

struct Paths {
    settings: PathBuf,
    usage: PathBuf,
    themes: PathBuf,
}

/// Everything behind one lock. Never hold it across a window call: showing or
/// hiding a window can deliver focus events on this same thread.
struct Core {
    engine: Engine,
    usage: Usage,
    themes: Vec<Theme>,
    settings: Settings,
}

struct AppState {
    core: Mutex<Core>,
    paths: Paths,
    timing: Mutex<Timing>,
    last_shown: Mutex<Instant>,
    /// Mirrors `settings.hide_on_blur` so focus events never need the core lock.
    hide_on_blur: AtomicBool,
    measuring_idle: AtomicBool,
    startup_problems: Vec<String>,
}

// ---------------------------------------------------------------- commands

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    theme: Theme,
    settings: Settings,
    /// Things that went wrong at startup, in words a user can act on.
    problems: Vec<String>,
}

#[tauri::command]
fn bootstrap(state: State<AppState>) -> Bootstrap {
    let core = state.core.lock().unwrap();
    Bootstrap {
        theme: current_theme(&core),
        settings: core.settings.clone(),
        problems: state.startup_problems.clone(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    hits: Vec<Hit>,
    /// Time spent inside the search engine, in microseconds.
    search_us: u64,
    indexed: usize,
}

#[tauri::command]
fn search(query: String, state: State<AppState>) -> SearchResponse {
    let mut core = state.core.lock().unwrap();
    let Core { engine, usage, .. } = &mut *core;
    let started = Instant::now();
    let hits = engine.search(&query, RESULT_LIMIT, usage, now_secs());
    SearchResponse {
        hits,
        search_us: started.elapsed().as_micros() as u64,
        indexed: engine.len(),
    }
}

#[tauri::command]
fn activate(id: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let action = {
        let mut core = state.core.lock().unwrap();
        let action = core
            .engine
            .get(&id)
            .map(|item| item.action.clone())
            .ok_or("That item is no longer available. Rescan applications and try again.")?;
        core.usage.record(&id, now_secs());
        // Losing usage history is not worth failing the launch over.
        let _ = core.usage.save();
        action
    };

    match action {
        Action::Open { path } => {
            hide_launcher(&app);
            open::that_detached(&path).map_err(|err| format!("Could not open {path}: {err}"))
        }
        Action::Command { name, arg } => run_command(&app, &state, &name, arg.as_deref()),
    }
}

#[tauri::command]
fn hide(app: AppHandle) {
    hide_launcher(&app);
}

/// The webview calls this from its first animation frame after being shown.
#[tauri::command]
fn first_frame(seq: u64, state: State<AppState>) -> HotkeyTimings {
    let mut timing = state.timing.lock().unwrap();
    timing.first_frame(seq);
    timing.report()
}

// ------------------------------------------------------- launcher commands

fn run_command(app: &AppHandle, state: &AppState, name: &str, arg: Option<&str>) -> Result<(), String> {
    match name {
        "theme.set" => {
            let id = arg.ok_or("No theme was named.")?;
            let theme = {
                let mut core = state.core.lock().unwrap();
                let theme = core
                    .themes
                    .iter()
                    .find(|theme| theme.id == id)
                    .cloned()
                    .ok_or_else(|| format!("There is no theme called {id}. Reload themes and try again."))?;
                core.settings.theme = theme.id.clone();
                save_settings(&core, state)?;
                theme
            };
            emit(app, "theme-changed", &theme);
            notice(app, format!("Using the {} theme", theme.name));
        }
        "themes.reload" => {
            let (theme, count, problems) = {
                let mut core = state.core.lock().unwrap();
                let (themes, problems) = theme::load_all(Some(&state.paths.themes));
                core.themes = themes;
                core.engine = Engine::new(build_items(&core.themes));
                (current_theme(&core), core.themes.len(), problems)
            };
            emit(app, "theme-changed", &theme);
            notice(
                app,
                match problems.first() {
                    Some(problem) => format!("Skipped a theme. {problem}"),
                    None => format!("Loaded {count} themes"),
                },
            );
        }
        "themes.open-folder" => {
            let dir = &state.paths.themes;
            std::fs::create_dir_all(dir).map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
            hide_launcher(app);
            open::that_detached(dir).map_err(|err| format!("Could not open {}: {err}", dir.display()))?;
        }
        "timings.toggle" => {
            let settings = {
                let mut core = state.core.lock().unwrap();
                core.settings.show_timings = !core.settings.show_timings;
                save_settings(&core, state)?;
                core.settings.clone()
            };
            emit(app, "settings-changed", &settings);
        }
        "apps.rescan" => {
            let count = {
                let mut core = state.core.lock().unwrap();
                core.engine = Engine::new(build_items(&core.themes));
                core.engine.len()
            };
            notice(app, format!("Indexed {count} items"));
        }
        "idle.measure" => measure_idle(app, state),
        "app.quit" => app.exit(0),
        other => return Err(format!("Unknown command {other}.")),
    }
    Ok(())
}

/// Hides the launcher, measures the whole process tree, then shows the result.
fn measure_idle(app: &AppHandle, state: &AppState) {
    if state.measuring_idle.swap(true, Ordering::SeqCst) {
        return;
    }
    hide_launcher(app);
    let app = app.clone();
    std::thread::spawn(move || {
        // Let the hide and any pending work finish before the window opens.
        std::thread::sleep(IDLE_SETTLE);
        let report = idle::measure(IDLE_WINDOW);
        let on_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            on_main.state::<AppState>().measuring_idle.store(false, Ordering::SeqCst);
            show_launcher(&on_main, None);
            notice(&on_main, report.sentence());
        });
    });
}

// ------------------------------------------------------------------ window

/// Shows the pre-warmed window. Must run on the main thread so the show call
/// has finished by the time it returns. `started` is when the hotkey fired.
fn show_launcher(app: &AppHandle, started: Option<Instant>) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
    // Focus the window, then the webview inside it, so typing lands in the
    // search box without a click.
    let _ = AsRef::<tauri::Webview>::as_ref(&window).set_focus();

    let state = app.state::<AppState>();
    *state.last_shown.lock().unwrap() = Instant::now();
    let seq = started.map(|started| state.timing.lock().unwrap().shown(started));
    emit(app, "launcher-shown", &seq);
}

fn hide_launcher(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    // Reset first, so the next show opens on a clean list instead of redrawing one.
    emit(app, "launcher-reset", &());
    let _ = window.hide();
}

fn toggle_launcher(app: &AppHandle) {
    let started = Instant::now();
    let on_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        let visible = on_main
            .get_webview_window(WINDOW)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if visible {
            hide_launcher(&on_main);
        } else {
            show_launcher(&on_main, Some(started));
        }
    });
}

/// Horizontally centred, a little above the middle of the primary display.
fn place_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    // The configured width, not the window's current one: a window that has
    // never been shown does not report a reliable size on every platform.
    let Some(logical_width) = app.config().app.windows.first().map(|config| config.width) else {
        return;
    };
    let width = (logical_width * monitor.scale_factor()) as i32;
    let screen = monitor.size();
    let origin = monitor.position();
    let x = origin.x + (screen.width as i32 - width) / 2;
    let y = origin.y + (screen.height as f64 * 0.2) as i32;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

// ----------------------------------------------------------------- helpers

fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: &T) {
    let _ = app.emit_to(WINDOW, event, payload.clone());
}

/// One line of feedback shown in the launcher's footer.
fn notice(app: &AppHandle, text: String) {
    emit(app, "notice", &text);
}

fn save_settings(core: &Core, state: &AppState) -> Result<(), String> {
    state.hide_on_blur.store(core.settings.hide_on_blur, Ordering::Relaxed);
    core.settings
        .save(&state.paths.settings)
        .map_err(|err| format!("Could not save settings to {}: {err}", state.paths.settings.display()))
}

fn current_theme(core: &Core) -> Theme {
    let by_id = |id: &str| core.themes.iter().find(|theme| theme.id == id);
    by_id(&core.settings.theme)
        .or_else(|| by_id(DEFAULT_THEME))
        .or(core.themes.first())
        .cloned()
        .expect("the bundled themes always load")
}

/// Installed applications plus the launcher's own commands, sorted by title.
fn build_items(themes: &[Theme]) -> Vec<Item> {
    let mut items = apps::discover();
    for theme in themes {
        items.push(Item::command(format!("Use {} theme", theme.name), "theme.set", Some(&theme.id)));
    }
    items.extend([
        Item::command("Reload themes", "themes.reload", None),
        Item::command("Open themes folder", "themes.open-folder", None),
        Item::command("Show or hide timings", "timings.toggle", None),
        Item::command("Measure idle CPU for 30 seconds", "idle.measure", None),
        Item::command("Rescan applications", "apps.rescan", None),
        Item::command("Quit Launcher", "app.quit", None),
    ]);
    sort_items(&mut items);
    items
}

// -------------------------------------------------------------------- main

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_launcher(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let config = app.path().app_config_dir()?;
            let data = app.path().app_data_dir()?;
            let paths = Paths {
                settings: config.join("settings.json"),
                themes: config.join("themes"),
                usage: data.join("usage.json"),
            };

            let settings = Settings::load(&paths.settings);
            if !paths.settings.exists() {
                // Put the defaults on disk so there is a file to edit.
                let _ = settings.save(&paths.settings);
            }
            let (themes, mut problems) = theme::load_all(Some(&paths.themes));
            let engine = Engine::new(build_items(&themes));

            if let Err(err) = app.global_shortcut().register(settings.hotkey.as_str()) {
                eprintln!("hotkey {} not registered: {err}", settings.hotkey);
                problems.push(format!(
                    "{} is not available as the hotkey. Change \"hotkey\" in {} and restart.",
                    settings.hotkey,
                    paths.settings.display()
                ));
            }
            let show_now = !problems.is_empty();

            app.manage(AppState {
                hide_on_blur: AtomicBool::new(settings.hide_on_blur),
                core: Mutex::new(Core { engine, usage: Usage::load(&paths.usage), themes, settings }),
                paths,
                timing: Mutex::new(Timing::default()),
                last_shown: Mutex::new(Instant::now()),
                measuring_idle: AtomicBool::new(false),
                startup_problems: problems,
            });

            // A launcher lives in the background: no Dock icon on macOS.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            place_window(app.handle());
            if show_now {
                // Without a working hotkey nothing else would ever show the window.
                show_launcher(app.handle(), None);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let settled = state.last_shown.lock().unwrap().elapsed() > BLUR_GRACE;
                if settled && state.hide_on_blur.load(Ordering::Relaxed) && window.is_visible().unwrap_or(false) {
                    hide_launcher(app);
                }
            }
            WindowEvent::CloseRequested { api, .. } => {
                // Alt+F4 hides rather than exits; "Quit Launcher" exits.
                api.prevent_close();
                hide_launcher(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![bootstrap, search, activate, hide, first_frame])
        .run(tauri::generate_context!())
        .expect("the launcher failed to start");
}
