//! The WebView2 processes behind the window (OVERHAUL_PLAN section 8, 2026-11-20, the
//! check-run hang). WebView2's browser process can die a few seconds after it starts when a
//! third-party program injected into every process crashes it (RivaTuner Statistics Server's
//! `RTSSHooks64.dll`, whose window hook faulted while the browser process created a COM
//! window). The webview cannot come back from that, and with the window transparent nothing
//! shows it: the UI is gone, the viewport stays, and a check run waited for its 600 s timer.
//!
//! - Browser process exited: the app starts itself again with the same arguments (at most
//!   [`RELAUNCHES`] times in a row). A check run hides its window, waits for the new run and
//!   exits with its code, so `--smoke` callers see one run; otherwise this process ends.
//! - A page's renderer process exited: the page is loaded again.
//! - Others (GPU, utility processes): WebView2 starts them again itself; logged.
//!
//! Every failure is written to standard error and to the stuck-state dump (`crate::diag`).
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::WebviewWindow;

/// Relaunches in a row before giving up (`OC_WEBVIEW_RELAUNCHES` counts them).
pub const RELAUNCHES: u32 = 2;

static RELAUNCHED: AtomicBool = AtomicBool::new(false);

/// This process handed its run to a relaunched one: its own timers must not report.
pub fn relaunched() -> bool {
    RELAUNCHED.load(Ordering::SeqCst)
}

/// What WebView2 says failed (`COREWEBVIEW2_PROCESS_FAILED_KIND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failed {
    BrowserExited,
    RenderExited,
    RenderUnresponsive,
    Other(i32),
}

impl Failed {
    pub fn from_kind(kind: i32) -> Failed {
        match kind {
            0 => Failed::BrowserExited,
            1 => Failed::RenderExited,
            2 => Failed::RenderUnresponsive,
            k => Failed::Other(k),
        }
    }
}

/// What to do about a failure: relaunch (with the count so far), reload or only log.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Relaunch(u32),
    GiveUp,
    Reload,
    Log,
}

pub fn action(f: Failed, relaunches_so_far: u32) -> Action {
    match f {
        Failed::BrowserExited if relaunches_so_far < RELAUNCHES => Action::Relaunch(relaunches_so_far + 1),
        Failed::BrowserExited => Action::GiveUp,
        Failed::RenderExited => Action::Reload,
        Failed::RenderUnresponsive | Failed::Other(_) => Action::Log,
    }
}

fn relaunches_so_far() -> u32 {
    std::env::var("OC_WEBVIEW_RELAUNCHES").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Watches the window's WebView2 processes. `check`: a `--smoke` run (`args` to report a
/// final failure with).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn watch(win: &WebviewWindow, check: Option<crate::smoke::Args>) {
    let w = win.clone();
    let r = win.with_webview(move |pw| {
        use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND;
        use webview2_com::ProcessFailedEventHandler;
        // SAFETY: WebView2 COM calls on the window's own controller, on its UI thread.
        let core = match unsafe { pw.controller().CoreWebView2() } {
            Ok(c) => c,
            Err(e) => return eprintln!("webview: cannot watch its processes: {e}"),
        };
        let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(a) = args {
                // SAFETY: as above.
                unsafe { a.ProcessFailedKind(&mut kind)? };
            }
            on_failed(&w, Failed::from_kind(kind.0), check.as_ref());
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: as above; the handler lives as long as the webview.
        if let Err(e) = unsafe { core.add_ProcessFailed(&handler, &mut token) } {
            eprintln!("webview: cannot watch its processes: {e}");
        }
    });
    if let Err(e) = r {
        eprintln!("webview: cannot watch its processes: {e}");
    }
}

#[cfg(not(windows))]
pub fn watch(_win: &WebviewWindow, _check: Option<crate::smoke::Args>) {}

fn on_failed(w: &WebviewWindow, f: Failed, check: Option<&crate::smoke::Args>) {
    let n = relaunches_so_far();
    let what = format!("webview: {f:?}");
    eprintln!("{what}");
    crate::diag::event(what);
    match action(f, n) {
        Action::Log => {}
        Action::Reload => {
            crate::diag::event("webview: page loaded again");
            let _ = w.reload();
        }
        Action::GiveUp => {
            let msg = format!("webview: its browser process exited again after {n} relaunches");
            eprintln!("{msg}");
            if let Some(args) = check {
                crate::smoke::emit(args, &format!("SMOKE {}", serde_json::json!({ "exception": msg, "stuck": { "native": crate::diag::dump() } })));
            }
            std::process::exit(4);
        }
        Action::Relaunch(k) => {
            RELAUNCHED.store(true, Ordering::SeqCst);
            crate::diag::event(format!("webview: browser process exited, the app starts again ({k} of {RELAUNCHES})"));
            eprintln!("webview: browser process exited; starting the app again ({k} of {RELAUNCHES})");
            let _ = w.hide();
            let wait = check.is_some();
            std::thread::spawn(move || {
                let code = match std::env::current_exe().and_then(|exe| {
                    std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("OC_WEBVIEW_RELAUNCHES", k.to_string()).spawn()
                }) {
                    Ok(mut child) if wait => child.wait().ok().and_then(|s| s.code()).unwrap_or(1),
                    Ok(_) => 0,
                    Err(e) => {
                        eprintln!("webview: could not start the app again: {e}");
                        4
                    }
                };
                std::process::exit(code);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_exit_relaunches_twice_then_gives_up() {
        assert_eq!(action(Failed::from_kind(0), 0), Action::Relaunch(1));
        assert_eq!(action(Failed::from_kind(0), 1), Action::Relaunch(2));
        assert_eq!(action(Failed::from_kind(0), 2), Action::GiveUp);
        assert_eq!(action(Failed::from_kind(1), 0), Action::Reload);
        assert_eq!(action(Failed::from_kind(2), 0), Action::Log);
        assert_eq!(action(Failed::from_kind(6), 0), Action::Log);
    }
}
