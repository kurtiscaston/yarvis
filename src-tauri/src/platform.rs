//! Windows behaviour the Win32 window does not get right on its own.
//!
//! Three things that already work on Linux fail with the window tao creates:
//!
//! * A window created hidden is shown with activation that Windows will not
//!   grant to a background process, so the webview never receives keys.
//! * Focusing that webview delivers `WM_KILLFOCUS` to the top-level window.
//!   tao treats focus as `is_active && is_focused`, so the later deactivation
//!   (clicking another app) no longer emits `Focused(false)` and the launcher
//!   stays up. The spurious `WM_KILLFOCUS` is also not a reason to hide.
//! * Acrylic is applied while the window is still hidden, and the undecorated
//!   shadow is a one-pixel white frame that covers the backdrop. Desktop
//!   Window Manager only shows the acrylic blur after the frame is extended
//!   into the client area of a visible window.

/// Hide when another window is in front, the launcher is actually showing,
/// and the show has settled. `foreground_is_ours` keeps a focus move into
/// our own webview from counting as a switch away.
pub fn should_hide(
    hide_on_blur: bool,
    visible: bool,
    settled: bool,
    foreground_is_ours: bool,
) -> bool {
    hide_on_blur && visible && settled && !foreground_is_ours
}

#[cfg(windows)]
mod win {
    use std::sync::OnceLock;

    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetAncestor, GetForegroundWindow,
        GetWindowThreadProcessId, IsChild, SetForegroundWindow, ShowWindow, ASFW_ANY,
        EVENT_SYSTEM_FOREGROUND, GA_ROOT, SW_SHOW, SW_SHOWNORMAL, WINEVENT_OUTOFCONTEXT,
    };

    use crate::on_foreground_changed;

    static HOOK: OnceLock<isize> = OnceLock::new();

    /// The top-level launcher and any HWND parented into it, including the
    /// WebView2 child that lives in another process.
    pub fn belongs_to_launcher(launcher: HWND, foreground: HWND) -> bool {
        if foreground.is_invalid() || launcher.is_invalid() {
            return false;
        }
        if foreground == launcher {
            return true;
        }
        unsafe {
            if IsChild(launcher, foreground).as_bool() {
                return true;
            }
            GetAncestor(foreground, GA_ROOT) == launcher
        }
    }

    /// Drop the undecorated shadow. On Windows that shadow is a one-pixel
    /// white frame and it paints over the acrylic backdrop.
    pub fn prepare(window: &WebviewWindow) {
        use tauri::window::Color;
        let _ = window.set_shadow(false);
        // Alpha 0 is the only value WebView2 treats as transparent.
        let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
        if let Ok(hwnd) = window.hwnd() {
            round_corners(hwnd);
        }
    }

    /// Show the pre-warmed window and put keyboard focus in its webview.
    /// Called on the hotkey's thread, which is the window thread, while
    /// Windows still allows this process to take the foreground.
    pub fn present(window: &WebviewWindow) {
        if let Ok(hwnd) = window.hwnd() {
            force_foreground(hwnd);
            // The backdrop only sticks once the window is visible, and
            // hiding clears it, so it is put back on every show.
            extend_frame(hwnd);
        }
        let _ = window.show();
        let _ = window.set_focus();
        let _ = AsRef::<tauri::Webview>::as_ref(window).set_focus();
        apply_acrylic(window);
    }

    pub fn install_foreground_hook() -> Result<(), String> {
        if HOOK.get().is_some() {
            return Ok(());
        }
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_hook),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // The hook has to live for the process; dropping the handle unhooks it.
        let _ = HOOK.set(hook.0 as isize);
        Ok(())
    }

    /// Open a Start Menu shortcut (or anything else the shell knows how to open).
    /// `ShellExecuteExW` resolves `.lnk` files; `CreateProcess` does not.
    /// `SEE_MASK_NOASYNC` makes a failed resolve return here instead of
    /// disappearing into the message loop after the launcher has hidden.
    pub fn launch(path: &str) -> std::io::Result<()> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpFile: windows::core::PCWSTR(wide.as_ptr()),
            nShow: SW_SHOWNORMAL.0 as i32,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            // The launched program is allowed to become the foreground window.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            ShellExecuteExW(&mut info).map_err(|err| std::io::Error::other(err.to_string()))
        }
    }

    fn apply_acrylic(window: &WebviewWindow) {
        use tauri::window::{Color, Effect, EffectState, EffectsBuilder};
        // Windows 10 uses this tint. Windows 11's backdrop ignores it and the
        // page's own translucent surface tints the blur instead.
        let _ = window.set_effects(
            EffectsBuilder::new()
                .effect(Effect::Acrylic)
                .state(EffectState::Active)
                .color(Color(22, 25, 51, 214))
                .build(),
        );
    }

    fn round_corners(hwnd: HWND) {
        let preference = DWMWCP_ROUND;
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const _ as *const _,
                std::mem::size_of_val(&preference) as u32,
            );
        }
    }

    fn extend_frame(hwnd: HWND) {
        // -1 makes the whole client area the glass frame, which is where
        // the system acrylic backdrop is allowed to show.
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        unsafe {
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
    }

    fn force_foreground(hwnd: HWND) {
        unsafe {
            let foreground = GetForegroundWindow();
            let foreground_thread = GetWindowThreadProcessId(foreground, None);
            let current = GetCurrentThreadId();
            let attached = foreground_thread != 0
                && foreground_thread != current
                && AttachThreadInput(foreground_thread, current, true).as_bool();

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = BringWindowToTop(hwnd);

            if attached {
                let _ = AttachThreadInput(foreground_thread, current, false);
            }
        }
    }

    unsafe extern "system" fn foreground_hook(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if event != EVENT_SYSTEM_FOREGROUND || hwnd.is_invalid() {
            return;
        }
        on_foreground_changed(hwnd.0 as isize);
    }
}

#[cfg(windows)]
pub use win::{belongs_to_launcher, install_foreground_hook, launch, prepare, present};

#[cfg(test)]
mod tests {
    use super::should_hide;

    #[test]
    fn hides_when_a_foreign_window_is_in_front_after_the_show_settles() {
        assert!(should_hide(true, true, true, false));
    }

    #[test]
    fn keeps_the_launcher_up_while_its_own_webview_holds_focus() {
        assert!(!should_hide(true, true, true, true));
    }

    #[test]
    fn ignores_focus_churn_during_the_grace_period_and_when_already_hidden() {
        assert!(!should_hide(true, true, false, false));
        assert!(!should_hide(true, false, true, false));
        assert!(!should_hide(false, true, true, false));
    }
}
