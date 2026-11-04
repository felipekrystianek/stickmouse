//! Checks the Stickmouse website once a day for a newer version. The app is not on the App Store,
//! so without this users would never hear about fixes. Nothing installs itself: the menu bar just
//! offers the download.

use std::cell::Cell;
use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{desktop, permissions};

const VERSION_URL: &str = "https://stickmouse.app/version.json";
const CHECK_EVERY: u64 = 24 * 60 * 60;
/// After a failed check (offline), try again this much later instead of waiting a whole day.
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
/// How often the main-thread timer looks at whether a check is due; reading a clock is enough.
const POLL_EVERY: Duration = Duration::from_secs(60);

/// What `version.json` on the website describes: the latest release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub notes: String,
}

/// Kept in the state directory so a restart neither checks again nor forgets a found update.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Stored {
    checked_at: Option<u64>,
    latest: Option<Release>,
}

static STORED: Mutex<Option<Stored>> = Mutex::new(None);
static CHECKING: AtomicBool = AtomicBool::new(false);
static FAILED_AT: Mutex<Option<Instant>> = Mutex::new(None);

thread_local! {
    static NEXT_POLL: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn file() -> std::path::PathBuf {
    desktop::state_dir().join("update.toml")
}

fn with_stored<T>(f: impl FnOnce(&mut Stored) -> T) -> T {
    let mut guard = STORED.lock().unwrap();
    let stored = guard.get_or_insert_with(|| {
        fs::read_to_string(file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
    });
    f(stored)
}

fn save(stored: &Stored) {
    let written = fs::create_dir_all(desktop::state_dir())
        .and_then(|()| toml::to_string(stored).map_err(std::io::Error::other))
        .and_then(|text| fs::write(file(), text));
    if let Err(error) = written {
        eprintln!("padcursor: could not save the update check: {error}");
    }
}

/// The newer release to offer, if the last check found one.
pub fn available() -> Option<Release> {
    with_stored(|stored| stored.latest.clone()).filter(|release| is_newer(&release.version, env!("CARGO_PKG_VERSION")))
}

/// Opens the download of the available release in the browser.
pub fn open_download() {
    if let Some(release) = available() {
        permissions::open(&release.url);
    }
}

/// Called on the main thread every few milliseconds; starts a background check when one is due.
pub fn check_if_due() {
    let due_to_poll = NEXT_POLL.get().is_none_or(|at| Instant::now() >= at);
    if !due_to_poll {
        return;
    }
    NEXT_POLL.set(Some(Instant::now() + POLL_EVERY));
    let retry_ok = FAILED_AT.lock().unwrap().is_none_or(|at| at.elapsed() >= RETRY_AFTER);
    let due = with_stored(|stored| stored.checked_at.is_none_or(|at| now() >= at + CHECK_EVERY));
    if !due || !retry_ok || CHECKING.swap(true, Ordering::Relaxed) {
        return;
    }
    thread::spawn(|| {
        match fetch() {
            Ok(release) => {
                *FAILED_AT.lock().unwrap() = None;
                with_stored(|stored| {
                    stored.checked_at = Some(now());
                    stored.latest = Some(release);
                    save(stored);
                });
            }
            // Offline, or the website is not live yet: stay quiet and try again later.
            Err(error) => {
                eprintln!("padcursor: could not check for updates: {error}");
                *FAILED_AT.lock().unwrap() = Some(Instant::now());
            }
        }
        CHECKING.store(false, Ordering::Relaxed);
    });
}

/// GETs `version.json` with the system curl, which handles proxies and certificates like the
/// rest of macOS. `-f` turns HTTP errors such as 404 into a failure.
fn fetch() -> Result<Release, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "-m", "20", VERSION_URL])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_release(&String::from_utf8_lossy(&output.stdout))
}

/// Reads `version.json`, refusing anything but an https download link, since it goes to `open`.
fn parse_release(text: &str) -> Result<Release, String> {
    let release: Release = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if parse_version(&release.version).is_none() {
        return Err(format!("bad version {:?}", release.version));
    }
    if !release.url.starts_with("https://") {
        return Err(format!("bad download link {:?}", release.url));
    }
    Ok(release)
}

/// "1.2.3" as numbers. A leading "v", a missing minor or patch, and a "-beta" or "+build"
/// suffix are accepted; the suffix is ignored.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_loosely() {
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("v1.10"), Some((1, 10, 0)));
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3+abc"), Some((1, 2, 3)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.x"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn version_json_parses_and_rejects_unsafe_links() {
        let release = parse_release(
            r#"{"version": "0.2.0", "url": "https://stickmouse.app/downloads/Stickmouse.dmg", "notes": "Fixes"}"#,
        )
        .unwrap();
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.notes, "Fixes");
        assert!(parse_release(r#"{"version": "0.2.0", "url": "https://x"}"#).is_ok());
        assert!(parse_release(r#"{"version": "0.2.0", "url": "file:///Applications/Calculator.app"}"#).is_err());
        assert!(parse_release(r#"{"version": "soon", "url": "https://x"}"#).is_err());
        assert!(parse_release("<html>404</html>").is_err());
    }

    #[test]
    fn stored_check_round_trips() {
        let stored = Stored {
            checked_at: Some(5),
            latest: Some(Release { version: "0.2.0".into(), url: "https://x".into(), notes: String::new() }),
        };
        let text = toml::to_string(&stored).unwrap();
        assert_eq!(toml::from_str::<Stored>(&text).unwrap(), stored);
    }
}
