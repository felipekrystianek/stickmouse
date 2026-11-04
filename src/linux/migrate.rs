//! Carries settings over from padcursor, Stickmouse's name on Linux before the rename.
//!
//! Old folders are copied rather than moved, so going back to an old build still finds them.

use std::fs;
use std::io;
use std::path::Path;

use super::cursor_theme::OLD_THEME_NAME;
use super::desktop::{self, APP_DIR};

/// The folder name padcursor used under the XDG config and state directories.
const OLD_APP_DIR: &str = "padcursor";
/// Belongs to the old copy's single-instance check; copying it would carry nothing useful.
const SKIPPED_FILES: [&str; 1] = ["lock"];

/// Copies padcursor's settings and state into Stickmouse's folders, unless those already exist.
/// Runs before the config is read and before the lock file creates the state folder.
pub fn old_settings() {
    for home in [desktop::config_home(), desktop::state_home()] {
        let (old, new) = (home.join(OLD_APP_DIR), home.join(APP_DIR));
        match copy_if_new(&old, &new) {
            Ok(true) => eprintln!("stickmouse: copied {} to {}", old.display(), new.display()),
            Ok(false) => {}
            Err(error) => eprintln!("stickmouse: could not copy {}: {error}", old.display()),
        }
    }
}

/// Removes padcursor's circle theme, unless it is still the active cursor. Call it after
/// `restore_cursor`, which switches away from the old circle when it can.
pub fn old_cursor_theme(icons_dir: &Path) {
    if desktop::current_theme().as_deref() == Some(OLD_THEME_NAME) {
        return;
    }
    if let Err(error) = remove_old_theme(icons_dir) {
        eprintln!("stickmouse: could not remove the old cursor theme: {error}");
    }
}

/// Copies the files directly inside `old` into a new folder `new`. Does nothing, and returns
/// false, if `old` is missing or `new` already exists, so earlier choices are never overwritten.
fn copy_if_new(old: &Path, new: &Path) -> io::Result<bool> {
    if !old.is_dir() || new.exists() {
        return Ok(false);
    }
    fs::create_dir_all(new)?;
    for entry in fs::read_dir(old)?.flatten() {
        let name = entry.file_name();
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_file && !SKIPPED_FILES.iter().any(|skipped| name == *skipped) {
            fs::copy(entry.path(), new.join(&name))?;
        }
    }
    Ok(true)
}

fn remove_old_theme(icons_dir: &Path) -> io::Result<()> {
    let old = icons_dir.join(OLD_THEME_NAME);
    if old.is_dir() { fs::remove_dir_all(old) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh, empty folder per test, so tests running in parallel never share files.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stickmouse-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_files_but_not_the_lock() {
        let home = scratch("copies");
        let (old, new) = (home.join(OLD_APP_DIR), home.join(APP_DIR));
        fs::create_dir(&old).unwrap();
        fs::write(old.join("enabled"), "false").unwrap();
        fs::write(old.join("original-cursor-theme"), "Adwaita").unwrap();
        fs::write(old.join("lock"), "").unwrap();

        assert!(copy_if_new(&old, &new).unwrap());
        assert_eq!(fs::read_to_string(new.join("enabled")).unwrap(), "false");
        assert_eq!(fs::read_to_string(new.join("original-cursor-theme")).unwrap(), "Adwaita");
        assert!(!new.join("lock").exists());
        assert!(old.join("enabled").exists(), "the old folder is kept");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn leaves_an_existing_new_folder_alone() {
        let home = scratch("existing");
        let (old, new) = (home.join(OLD_APP_DIR), home.join(APP_DIR));
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        fs::write(old.join("config.toml"), "old").unwrap();
        fs::write(new.join("config.toml"), "new").unwrap();

        assert!(!copy_if_new(&old, &new).unwrap());
        assert_eq!(fs::read_to_string(new.join("config.toml")).unwrap(), "new");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn does_nothing_without_an_old_folder() {
        let home = scratch("missing");
        let new = home.join(APP_DIR);
        assert!(!copy_if_new(&home.join(OLD_APP_DIR), &new).unwrap());
        assert!(!new.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn removes_only_the_old_theme() {
        let icons = scratch("theme");
        fs::create_dir_all(icons.join(OLD_THEME_NAME).join("cursors")).unwrap();
        fs::create_dir(icons.join("breeze_cursors")).unwrap();

        remove_old_theme(&icons).unwrap();
        assert!(!icons.join(OLD_THEME_NAME).exists());
        assert!(icons.join("breeze_cursors").exists());
        remove_old_theme(&icons).unwrap();
        fs::remove_dir_all(icons).unwrap();
    }
}
