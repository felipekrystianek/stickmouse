//! Stickmouse is paid: a 7-day trial, then a license key sold through Polar (polar.sh).
//! Keys are activated and checked against Polar's public customer-portal API, which needs no
//! secret, so nothing private is built into the app.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::desktop;

/// From the Polar dashboard: Settings, General, Organization ID.
const ORGANIZATION_ID: &str = "4b4ecb33-daab-4e43-b968-a2f94190d8e7";
/// Where the "Buy" button goes: the Stickmouse website, or a Polar checkout link.
pub const BUY_URL: &str = "https://buy.polar.sh/polar_cl_Z7wMa7FaNDxNY8MoMiqM6UTXGiVWJQPU1vT2J0MzuH6";
pub const PRICE: &str = "$5";
const API: &str = "https://api.polar.sh/v1/customer-portal/license-keys";
const TRIAL_DAYS: u64 = 7;
const DAY: u64 = 24 * 60 * 60;
/// How often a stored key is checked again, to notice refunds. Offline is fine in between.
const REVALIDATE_EVERY: u64 = 7 * DAY;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Stored {
    trial_started: Option<u64>,
    key: Option<String>,
    activation_id: Option<String>,
    validated_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Licensed,
    Trial { days_left: u64 },
    Expired,
}

static STORED: Mutex<Option<Stored>> = Mutex::new(None);
/// What the license window shows under the key field, such as an activation error.
static MESSAGE: Mutex<Option<String>> = Mutex::new(None);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn file() -> std::path::PathBuf {
    desktop::state_dir().join("license.toml")
}

fn with_stored<T>(f: impl FnOnce(&mut Stored) -> T) -> T {
    let mut guard = STORED.lock().unwrap();
    let stored = guard.get_or_insert_with(|| {
        let mut stored: Stored = fs::read_to_string(file())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        if stored.trial_started.is_none() {
            stored.trial_started = Some(now());
            save(&stored);
        }
        stored
    });
    f(stored)
}

fn save(stored: &Stored) {
    let written = toml::to_string(stored)
        .map_err(std::io::Error::other)
        .and_then(|text| fs::write(file(), text));
    if let Err(error) = written {
        eprintln!("stickmouse: could not save the license: {error}");
    }
}

fn status_at(stored: &Stored, now: u64) -> Status {
    if stored.key.is_some() && stored.activation_id.is_some() {
        return Status::Licensed;
    }
    let started = stored.trial_started.unwrap_or(now);
    let used = now.saturating_sub(started) / DAY;
    if used < TRIAL_DAYS { Status::Trial { days_left: TRIAL_DAYS - used } } else { Status::Expired }
}

pub fn status() -> Status {
    with_stored(|stored| status_at(stored, now()))
}

/// Whether controller input may drive the Mac.
pub fn allows_input() -> bool {
    status() != Status::Expired
}

/// The key with all but the last group hidden, for showing in the window.
pub fn masked_key() -> Option<String> {
    with_stored(|stored| {
        stored.key.as_ref().map(|key| {
            let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("••••-{tail}")
        })
    })
}

pub fn message() -> Option<String> {
    MESSAGE.lock().unwrap().clone()
}

fn set_message(text: impl Into<String>) {
    *MESSAGE.lock().unwrap() = Some(text.into());
}

/// Activates `key` for this Mac in the background. Watch `status()` and `message()` for the result.
pub fn activate(key: String) {
    let key = key.trim().to_string();
    if key.is_empty() {
        set_message("Paste the license key from your purchase email.");
        return;
    }
    if ORGANIZATION_ID.starts_with("00000000") {
        set_message("Licensing is not set up in this build yet.");
        return;
    }
    set_message("Checking your key…");
    thread::spawn(move || {
        let body = serde_json::json!({
            "key": key,
            "organization_id": ORGANIZATION_ID,
            "label": computer_name(),
        });
        match post("activate", &body) {
            Ok((200, response)) => {
                let Some(id) = response["id"].as_str() else {
                    set_message("Unexpected answer from the license server. Try again later.");
                    return;
                };
                with_stored(|stored| {
                    stored.key = Some(key);
                    stored.activation_id = Some(id.to_string());
                    stored.validated_at = Some(now());
                    save(stored);
                });
                set_message("Thank you! Stickmouse is activated.");
            }
            Ok((403, _)) => set_message(
                "This key is already used on the maximum number of Macs, or it was refunded.",
            ),
            Ok((404 | 422, _)) => set_message("That key was not found. Check it and try again."),
            Ok((code, _)) => set_message(format!("The license server answered {code}. Try again later.")),
            Err(error) => set_message(format!("Could not reach the license server: {error}")),
        }
    });
}

/// Checks a stored key again once a week, and forgets it if it was refunded or revoked.
pub fn revalidate_if_due() {
    let due = with_stored(|stored| match (&stored.key, &stored.activation_id) {
        (Some(key), Some(activation)) if stored.validated_at.is_none_or(|at| now() >= at + REVALIDATE_EVERY) => {
            Some((key.clone(), activation.clone()))
        }
        _ => None,
    });
    let Some((key, activation)) = due else {
        return;
    };
    thread::spawn(move || {
        let body = serde_json::json!({
            "key": key,
            "organization_id": ORGANIZATION_ID,
            "activation_id": activation,
        });
        match post("validate", &body) {
            Ok((200, _)) => with_stored(|stored| {
                stored.validated_at = Some(now());
                save(stored);
            }),
            Ok((404, _)) => {
                eprintln!("stickmouse: the license key is no longer valid");
                with_stored(|stored| {
                    forget_key(stored);
                    save(stored);
                });
                set_message("Your license key is no longer valid.");
            }
            // Offline or a server problem: keep working and try again next launch.
            other => eprintln!("stickmouse: could not check the license: {other:?}"),
        }
    });
}

/// Frees this Mac's activation in the background, so the key can be activated on another Mac.
/// Watch `status()` and `message()` for the result.
pub fn deactivate() {
    let activation = with_stored(|stored| stored.key.clone().zip(stored.activation_id.clone()));
    let Some((key, activation)) = activation else {
        set_message("Stickmouse is not activated on this Mac.");
        return;
    };
    set_message("Deactivating…");
    thread::spawn(move || {
        let body = serde_json::json!({
            "key": key,
            "organization_id": ORGANIZATION_ID,
            "activation_id": activation,
        });
        match post("deactivate", &body) {
            // 404: the activation is already gone (for example removed in the customer portal),
            // which leaves this Mac in the state the user asked for.
            Ok((204 | 404, _)) => {
                with_stored(|stored| {
                    forget_key(stored);
                    save(stored);
                });
                set_message("Stickmouse is deactivated on this Mac. You can now activate it on another one.");
            }
            Ok((code, _)) => set_message(format!("The license server answered {code}. Try again later.")),
            // Keep the license: forgetting it offline would leave the activation slot used up.
            Err(error) => set_message(format!("Could not reach the license server: {error}")),
        }
    });
}

/// Drops the key but keeps `trial_started`, so deactivating never grants a fresh trial.
fn forget_key(stored: &mut Stored) {
    stored.key = None;
    stored.activation_id = None;
    stored.validated_at = None;
}

/// POSTs JSON with the system curl, which handles proxies and certificates like the rest of macOS.
fn post(endpoint: &str, body: &serde_json::Value) -> Result<(u16, serde_json::Value), String> {
    let mut child = Command::new("curl")
        .args(["-sS", "-m", "20", "-X", "POST", "-H", "Content-Type: application/json"])
        .args(["--data-binary", "@-", "-w", "\n%{http_code}"])
        .arg(format!("{API}/{endpoint}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.to_string().as_bytes()).map_err(|error| error.to_string())?;
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_response(&String::from_utf8_lossy(&output.stdout))
}

/// Splits curl's output into the JSON body and the status code that `-w` appends. Some answers,
/// such as 204 after deactivating, have no body; that becomes `Null`.
fn parse_response(text: &str) -> Result<(u16, serde_json::Value), String> {
    let (json, code) = text.rsplit_once('\n').ok_or("empty answer")?;
    let code = code.trim().parse().map_err(|_| "no status code")?;
    let body = if json.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(json).unwrap_or(serde_json::Value::Null)
    };
    Ok((code, body))
}

fn computer_name() -> String {
    Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial(started_days_ago: u64) -> Stored {
        Stored { trial_started: Some(100 * DAY - started_days_ago * DAY), ..Stored::default() }
    }

    #[test]
    fn trial_counts_down_then_expires() {
        assert_eq!(status_at(&trial(0), 100 * DAY), Status::Trial { days_left: 7 });
        assert_eq!(status_at(&trial(6), 100 * DAY), Status::Trial { days_left: 1 });
        assert_eq!(status_at(&trial(7), 100 * DAY), Status::Expired);
    }

    #[test]
    fn an_activated_key_unlocks_after_the_trial() {
        let stored = Stored {
            key: Some("KEY".into()),
            activation_id: Some("id".into()),
            ..trial(30)
        };
        assert_eq!(status_at(&stored, 100 * DAY), Status::Licensed);
    }

    #[test]
    fn a_key_without_activation_does_not_unlock() {
        let stored = Stored { key: Some("KEY".into()), ..trial(30) };
        assert_eq!(status_at(&stored, 100 * DAY), Status::Expired);
    }

    #[test]
    fn forgetting_the_key_keeps_the_trial_start() {
        let mut stored = Stored {
            key: Some("KEY".into()),
            activation_id: Some("id".into()),
            validated_at: Some(5),
            ..trial(30)
        };
        forget_key(&mut stored);
        assert_eq!(stored, trial(30));
        assert_eq!(status_at(&stored, 100 * DAY), Status::Expired);
    }

    #[test]
    fn responses_parse_with_and_without_a_body() {
        let (code, body) = parse_response("{\"id\":\"abc\"}\n200").unwrap();
        assert_eq!((code, body["id"].as_str()), (200, Some("abc")));
        assert_eq!(parse_response("\n204").unwrap(), (204, serde_json::Value::Null));
        assert!(parse_response("204").is_err());
        assert!(parse_response("{}\nxyz").is_err());
    }

    #[test]
    fn stored_license_round_trips() {
        let stored = Stored { key: Some("A-B".into()), validated_at: Some(5), ..trial(1) };
        let text = toml::to_string(&stored).unwrap();
        assert_eq!(toml::from_str::<Stored>(&text).unwrap(), stored);
    }
}
