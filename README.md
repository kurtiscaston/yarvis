# Yarvis

Yarvis is a keyboard launcher for Windows and macOS. This is its first spike:
a small build to answer one question before anything else is built on it. Is
Tauri 2 fast enough?

It is a real launcher, not a mock-up. A hotkey shows a window that was loaded
at startup and kept hidden, typing searches your installed apps in Rust, and
Enter opens the selected one. It ships three themes and reads its own timings.

## Run it

You need [Rust](https://rustup.rs) and Node 20 or newer. On Windows you also
need the Microsoft C++ Build Tools (the Rust installer offers them) and
WebView2, which Windows 11 includes.

```
npm install
npm run app            # development build, reloads the UI as you edit
npm run app:release    # optimized build, no installer
```

The release build lands in `target/release/` as `yarvis` (`.exe` on
Windows). **Take timings from the release build only**; a development build is
several times slower.

Press **Ctrl+Shift+Space** to show and hide the launcher. To use another
hotkey, change `hotkey` in `settings.json` and restart. The file sits one
level above the folder that "Open themes folder" opens (on Windows,
`%APPDATA%\io.github.kurtiscaston.yarvis`). If the hotkey is already taken by another
app, the launcher opens at startup and says so.

## What to try

- Type part of an app's name and press Enter.
- Run "Use Celadon theme" or "Use Cobalt theme".
- Run "Open themes folder", copy one of the files in `themes/` there, change
  a colour, then run "Reload themes".
- Run "Measure idle CPU for 30 seconds". The window hides, then comes back
  with the result.
- "Quit Yarvis" exits. There is no tray icon yet.

## Reading the timings

The footer shows four numbers, all in milliseconds. Pairs are median / 95th
percentile since the app started.

| Readout | What it measures | Budget |
|---|---|---|
| Search | Time inside the search engine for the last query | under 1 |
| Typing | Key press to the start of the frame that paints its results | p95 within one display refresh (16.7 at 60 Hz) |
| Shown | Hotkey to the OS call that shows the window returning | p95 under 20, together with the next line |
| First frame | Hotkey to the webview drawing its first frame after showing | p95 under 20 |

"Measure idle CPU" has its own budget: under 0.1% of one core.

Two limits on what these numbers mean:

- They are taken inside the app. They leave out the time the OS takes to
  deliver the key press, and the last step from frame to photons. A true
  black-box figure needs an outside tool; that comes later if these are close.
- On macOS the idle measurement counts only the main process, because WebKit's
  helper processes are started by the system. Use Activity Monitor there.

Use the launcher twenty or thirty times before reading the pairs. "Show or
hide timings" turns the readout off.

## What is in here

```
crates/core/   Search, app discovery, frecency, themes, settings. No UI code.
src-tauri/     The Tauri shell: window, hotkey, IPC commands, timing.
src/           The UI (Svelte 5). Draws what the core returns.
themes/        The bundled themes.
```

The core does not depend on Tauri. If the timings say the webview is the
problem, the core moves to a native front end unchanged.

```
npm test         # Rust unit tests for the core and the shell
npm run bench    # search engine alone, 5,000 items
npm run check    # type-check the UI
npm run dev      # the UI alone in a browser, with sample data
```

## Themes

A theme is a JSON file of design tokens. Every token is optional; missing ones
fall back to Dusk.

```json
{
  "name": "Dusk",
  "appearance": "dark",
  "tokens": {
    "surface": "rgba(22, 25, 51, 0.84)",
    "tile": "rgba(237, 232, 220, 0.09)",
    "text": "#EDE8DC",
    "muted": "#9A9DC0",
    "rule": "rgba(237, 232, 220, 0.12)",
    "select": "#F2B33D",
    "select-text": "#161933",
    "caret": "#F2B33D"
  }
}
```

Values must be plain CSS values. Anything that could load a file or escape the
declaration (`url(...)`, `;`, braces) is refused, so a shared theme cannot do
more than restyle.

## Not built yet

- **macOS panel behaviour.** The window is an ordinary window, so showing it
  takes focus from the app you were in. Raycast-style behaviour needs an
  `NSPanel`; that is the next macOS spike.
- **Store apps on Windows.** Only Start Menu shortcuts are indexed.
- **App icons.** Rows show a letter tile.
- **Multiple displays.** The window always opens on the primary display.
- Plugins, AI providers, file search, clipboard history, tray icon, installer,
  auto-update.

## What has and has not been checked

Checked on Linux, where this was written: the unit tests, the UI in a browser,
and the whole app on a virtual display (hotkey, typing, opening a command,
theme switch, hide on focus loss, idle measurement). The Windows-only and
macOS-only app discovery code was type-checked but not run.

Not checked, because it needs a real Windows or macOS machine: the blurred
transparent window, keyboard focus arriving on hotkey, opening `.lnk`
shortcuts, and every timing that matters.
