//! One Analyser at a time. A second launch (the family switcher in the
//! Player, Library or Equaliser, or a double-click) asks the Analyser
//! that's already running to come to the front, then exits, rather than
//! opening a second one on top of the first.
//!
//! The family's way of doing it, the same as the Player's (see TAW-Panel's
//! FAMILY.md): the running app writes `running` (its process id and the
//! time, in milliseconds) to its data folder every second, and takes
//! requests from `inbox/*.json` there. Files, not a socket, so nothing
//! listens on the network.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `%APPDATA%\Timster's Audio Works\Analyser` on Windows,
/// `~/Library/Application Support/Timster's Audio Works/Analyser` on
/// macOS, `~/.local/share/timsters-audio-works/analyser` elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| {
            PathBuf::from(a)
                .join("Timster's Audio Works")
                .join("Analyser")
        })
    } else if cfg!(target_os = "macos") {
        home().map(|h| {
            h.join("Library/Application Support")
                .join("Timster's Audio Works")
                .join("Analyser")
        })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".local/share")))
            .map(|d| d.join("timsters-audio-works").join("analyser"))
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The process id of another Analyser that checked in within the last
/// few seconds.
fn other_running(dir: &Path) -> Option<u32> {
    let s = std::fs::read_to_string(dir.join("running")).ok()?;
    let mut it = s.split_whitespace();
    let pid: u32 = it.next()?.parse().ok()?;
    let at: u128 = it.next()?.parse().ok()?;
    (pid != std::process::id() && now_ms().saturating_sub(at) < 3_000).then_some(pid)
}

/// If another Analyser is running, asks it to come to the front and
/// returns true (this launch can then exit). False if there's none, or it
/// didn't answer within three seconds: open as usual.
pub fn hand_over(dir: &Path) -> bool {
    let Some(pid) = other_running(dir) else {
        return false;
    };
    allow_to_front(pid);
    let inbox = dir.join("inbox");
    let _ = std::fs::create_dir_all(&inbox);
    let file = inbox.join(format!("{}-{}.json", now_ms(), std::process::id()));
    let tmp = file.with_extension("tmp");
    // The family's request: files to open (none, here) and whether to play.
    let body = r#"{"paths":[],"play":false}"#;
    if std::fs::write(&tmp, body).is_err() || std::fs::rename(&tmp, &file).is_err() {
        return false;
    }
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if !file.exists() {
            return true;
        }
    }
    // It didn't take it: open here instead.
    std::fs::remove_file(&file).is_err()
}

/// Windows only lets the app the user is using bring another to the front;
/// this launch is that app for now, so it passes the right on.
#[cfg(windows)]
fn allow_to_front(pid: u32) {
    // SAFETY: a plain Win32 call with no pointers.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
}

#[cfg(not(windows))]
fn allow_to_front(_pid: u32) {}

/// Takes the requests waiting in the inbox; true if there were any.
fn take_requests(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir.join("inbox")) else {
        return false;
    };
    let mut any = false;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") {
            let _ = std::fs::remove_file(&p);
            any = true;
        }
    }
    any
}

/// Checks in every second and brings the window to the front when another
/// launch asks, on a thread of its own, so it works however rarely the
/// panel redraws.
pub fn watch(dir: PathBuf, ctx: egui::Context) {
    let _ = std::fs::create_dir_all(dir.join("inbox"));
    // Requests left from before this one started aren't for it.
    take_requests(&dir);
    let _ = std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            let mut last_beat = 0u128;
            loop {
                let t = now_ms();
                if t - last_beat >= 1_000 {
                    last_beat = t;
                    let _ = std::fs::write(
                        dir.join("running"),
                        format!("{} {}", std::process::id(), t),
                    );
                }
                if take_requests(&dir) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("taw-an-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("inbox")).unwrap();
        d
    }

    #[test]
    fn a_second_launch_asks_the_first() {
        let d = temp_dir("instance");
        // Nobody running: open as usual.
        assert!(!hand_over(&d));
        // Our own beacon doesn't count.
        std::fs::write(
            d.join("running"),
            format!("{} {}", std::process::id(), now_ms()),
        )
        .unwrap();
        assert!(other_running(&d).is_none());
        // Another's fresh beacon does; a stale one doesn't.
        std::fs::write(d.join("running"), format!("1 {}", now_ms())).unwrap();
        assert_eq!(other_running(&d), Some(1));
        std::fs::write(d.join("running"), format!("1 {}", now_ms() - 10_000)).unwrap();
        assert!(other_running(&d).is_none());
        // A request is taken once.
        std::fs::write(d.join("inbox/1-1.json"), "{}").unwrap();
        assert!(take_requests(&d));
        assert!(!take_requests(&d));
        let _ = std::fs::remove_dir_all(&d);
    }
}
