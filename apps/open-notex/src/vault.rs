//! Choosing, checking and remembering the vault folder.
//!
//! Everything here is synchronous and takes its directories as arguments, so a
//! test can point it at a temp home without touching the real one. The view does
//! none of this: `setup` resolves the launch, `poll` answers the dialog, and the
//! only thing that reaches the window is [`notex_ui::VaultStatus`].

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use notex_ui::VaultStatus;

/// What a launch should do about the vault, before the first frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// A folder that exists: open it, and remember it.
    Open(PathBuf),
    /// Nothing usable: ask, pre-filled at `path`.
    Ask {
        path: PathBuf,
        notice: Option<String>,
    },
}

/// `~/Documents/Notes`, the folder offered on first launch.
pub fn suggested_vault(home: &Path) -> PathBuf {
    home.join("Documents").join("Notes")
}

/// The choice lives beside the database, never in the vault (rule 8).
pub fn config_path(app_support: &Path) -> PathBuf {
    app_support.join("config.json")
}

/// Decide what launch does. The order matters, and a saved path is never
/// silently replaced by the default: an unmounted drive must not switch vaults.
pub fn resolve(home: &Path, app_support: &Path) -> Launch {
    let suggested = suggested_vault(home);
    match read_config(app_support) {
        Ok(Some(path)) if path.is_dir() => Launch::Open(path),
        Ok(Some(path)) if path.exists() => {
            // A file where a folder was. The dialog names it; keep the path so
            // the person sees what is wrong instead of a stranger's default.
            Launch::Ask { path, notice: None }
        }
        Ok(Some(path)) => Launch::Ask {
            path,
            notice: Some("That folder is not there. Is the drive connected?".to_owned()),
        },
        // No saved choice: a familiar folder is a good guess, and only if it is
        // already there. Creating one without asking would be a surprise.
        Ok(None) if suggested.is_dir() => Launch::Open(suggested),
        Ok(None) => Launch::Ask {
            path: suggested,
            notice: None,
        },
        Err(error) => Launch::Ask {
            path: suggested,
            notice: Some(format!(
                "The saved vault setting could not be read: {error}"
            )),
        },
    }
}

/// What the folder actually is, for the dialog's one line.
///
/// `path` is a `str` because that is what the view holds; an empty or relative
/// path is refused rather than resolved against the process's directory.
pub fn inspect(path: &str, app_support: &Path) -> VaultStatus {
    let path = Path::new(path);
    if path.as_os_str().is_empty() {
        return VaultStatus::Unusable("Choose a folder first.".to_owned());
    }
    if !path.is_absolute() {
        return VaultStatus::Unusable(
            "That is not a full path. It has to start with /.".to_owned(),
        );
    }
    if inside(path, app_support) {
        return VaultStatus::Unusable(
            "That folder holds openNoteX's own data. Pick another one.".to_owned(),
        );
    }
    if path.exists() && !path.is_dir() {
        return VaultStatus::Unusable("That is a file, not a folder.".to_owned());
    }
    if !path.exists() {
        return VaultStatus::Missing;
    }
    if let Err(reason) = writable(path) {
        return VaultStatus::Unusable(reason);
    }
    match notex_store::vault::scan_paths(path) {
        Ok(paths) if paths.is_empty() => VaultStatus::Empty,
        Ok(paths) => VaultStatus::Notes(paths.len()),
        Err(error) => VaultStatus::Unusable(format!("That folder could not be read: {error}")),
    }
}

/// Make sure `path` can be opened, creating it when the dialog's primary button
/// said "Create folder". Everything the dialog refused is refused here too, so a
/// stale frame cannot slip past.
pub fn prepare(path: &Path, app_support: &Path, create: bool) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("That is not a full path. It has to start with /.".to_owned());
    }
    if inside(path, app_support) {
        return Err("That folder holds openNoteX's own data. Pick another one.".to_owned());
    }
    if path.exists() {
        if !path.is_dir() {
            return Err("That is a file, not a folder.".to_owned());
        }
    } else if create {
        fs::create_dir_all(path)
            .map_err(|error| format!("That folder could not be created: {error}"))?;
    } else {
        return Err("This folder does not exist yet.".to_owned());
    }
    writable(path)
}

/// Read the remembered vault. `Ok(None)` is "never chosen", which is not an
/// error; a config that exists but cannot be understood is.
pub fn read_config(app_support: &Path) -> Result<Option<PathBuf>, String> {
    let path = config_path(app_support);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    match value.get("vault").and_then(serde_json::Value::as_str) {
        Some(vault) => Ok(Some(PathBuf::from(vault))),
        None => Err("it has no vault path".to_owned()),
    }
}

/// Write the choice atomically: a crash leaves the old config or the new one,
/// never half of either.
pub fn remember(app_support: &Path, vault: &Path) -> Result<(), String> {
    fs::create_dir_all(app_support).map_err(|error| error.to_string())?;
    let temp = app_support.join("config.json.tmp");
    let body = serde_json::json!({ "vault": vault.to_string_lossy() }).to_string();
    fs::write(&temp, body).map_err(|error| error.to_string())?;
    fs::rename(&temp, config_path(app_support)).map_err(|error| error.to_string())
}

/// Whether `path` is `dir` or lives inside it. The index and its database must
/// never share a folder (rule 8), and a vault there would index the app's own
/// files.
fn inside(path: &Path, dir: &Path) -> bool {
    if path.starts_with(dir) {
        return true;
    }
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(path), Ok(dir)) => path.starts_with(dir),
        _ => false,
    }
}

/// A read-only folder is refused before anything is indexed, with the sentence
/// the dialog shows. The probe is one empty hidden file, removed immediately.
fn writable(dir: &Path) -> Result<(), String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let probe = dir.join(format!(".open-notex-write-{}-{stamp}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            Err("That folder is read-only.".to_owned())
        }
        Err(error) => Err(format!("That folder cannot be written to: {error}")),
    }
}
