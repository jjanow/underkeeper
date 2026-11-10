//! Finding the game install and the save folders on Windows, Linux (native
//! Steam, Flatpak, Snap, Proton) and macOS (CrossOver / Whisky / Wine bottles),
//! for both the Steam and the GOG versions of the game.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "2411200";

/// A folder that holds Underkeep save files.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveDir {
    pub path: PathBuf,
    /// Short human-readable origin, e.g. "Steam Cloud (user 1234)".
    pub label: String,
}

fn push_unique(v: &mut Vec<PathBuf>, p: PathBuf) {
    let canon = std::fs::canonicalize(&p).unwrap_or(p);
    if !v.contains(&canon) {
        v.push(canon);
    }
}

/// Wine/CrossOver/Whisky prefixes that might contain a Windows Steam install.
fn wine_prefixes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    out.push(home.join(".wine"));
    let bottle_parents = [
        home.join("Library/Application Support/CrossOver/Bottles"),
        home.join("Library/Containers/com.isaacmarovitz.Whisky/Bottles"),
        home.join("Library/Application Support/com.isaacmarovitz.Whisky/Bottles"),
        home.join("Library/Containers/com.heroicgameslauncher.hgl/Prefixes"),
        home.join("Games/Heroic/Prefixes"),
        home.join(".local/share/bottles/bottles"),
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        home.join("Games"),
    ];
    for parent in bottle_parents {
        if let Ok(rd) = std::fs::read_dir(&parent) {
            for e in rd.flatten() {
                let p = e.path();
                if p.join("drive_c").is_dir() {
                    out.push(p);
                } else if p.join("pfx/drive_c").is_dir() {
                    out.push(p.join("pfx"));
                }
            }
        }
    }
    out
}

/// Candidate Steam installation roots for the current platform.
pub fn steam_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                push_unique(&mut out, PathBuf::from(p));
            }
        }
        for sub in [
            "SOFTWARE\\WOW6432Node\\Valve\\Steam",
            "SOFTWARE\\Valve\\Steam",
        ] {
            if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) {
                if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                    push_unique(&mut out, PathBuf::from(p));
                }
            }
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(pf) = std::env::var(var) {
                push_unique(&mut out, PathBuf::from(pf).join("Steam"));
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".steam/steam",
            ".steam/root",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "snap/steam/common/.local/share/Steam",
            "Library/Application Support/Steam",
        ] {
            let p = home.join(rel);
            if p.is_dir() {
                push_unique(&mut out, p);
            }
        }
    }

    for prefix in wine_prefixes() {
        for rel in [
            "drive_c/Program Files (x86)/Steam",
            "drive_c/Program Files/Steam",
        ] {
            let p = prefix.join(rel);
            if p.is_dir() {
                push_unique(&mut out, p);
            }
        }
    }

    out.retain(|p| p.is_dir());
    out
}

/// Steam library folders listed in `libraryfolders.vdf`, plus the root itself.
fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    for vdf in [
        root.join("steamapps/libraryfolders.vdf"),
        root.join("config/libraryfolders.vdf"),
    ] {
        let Ok(text) = std::fs::read_to_string(&vdf) else {
            continue;
        };
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            // "path"		"D:\\SteamLibrary"
            if parts.len() >= 4 && parts[1] == "path" {
                let raw = parts[3].replace("\\\\", "\\");
                let mut p = PathBuf::from(&raw);
                // A Windows path read by a native (non-Windows) build inside a
                // Wine bottle: map "C:\..." onto the bottle's drive_c.
                if !cfg!(windows)
                    && raw.len() > 2
                    && &raw[1..2] == ":"
                    && let Some(drive_c) = root.ancestors().find(|a| a.ends_with("drive_c"))
                    && raw[..1].eq_ignore_ascii_case("c")
                {
                    p = drive_c.join(raw[3..].replace('\\', "/"));
                }
                if p.is_dir() {
                    push_unique(&mut out, p);
                }
            }
        }
    }
    out
}

/// Candidate GOG install folders for Underkeep (not yet checked for `base/`).
fn gog_game_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        // GOG Galaxy and the offline installers register every game here.
        for sub in [
            "SOFTWARE\\WOW6432Node\\GOG.com\\Games",
            "SOFTWARE\\GOG.com\\Games",
        ] {
            if let Ok(games) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) {
                for id in games.enum_keys().flatten() {
                    if let Ok(k) = games.open_subkey(&id) {
                        if let Ok(p) = k.get_value::<String, _>("path") {
                            push_unique(&mut out, PathBuf::from(p));
                        }
                    }
                }
            }
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(pf) = std::env::var(var) {
                push_unique(
                    &mut out,
                    PathBuf::from(pf).join("GOG Galaxy/Games/Underkeep"),
                );
            }
        }
        push_unique(&mut out, PathBuf::from("C:\\GOG Games\\Underkeep"));
    }

    // Heroic installs GOG games natively (Linux) or into ~/Games/Heroic.
    if let Some(home) = dirs::home_dir() {
        for rel in ["Games/Heroic/Underkeep", "GOG Games/Underkeep"] {
            out.push(home.join(rel));
        }
    }

    for prefix in wine_prefixes() {
        for rel in [
            "drive_c/Program Files (x86)/GOG Galaxy/Games/Underkeep",
            "drive_c/Program Files/GOG Galaxy/Games/Underkeep",
            "drive_c/GOG Games/Underkeep",
        ] {
            out.push(prefix.join(rel));
        }
    }
    out
}

/// The Underkeep install directory (the folder holding `base/`), if found.
pub fn find_game_dir() -> Option<PathBuf> {
    for root in steam_roots() {
        for lib in library_folders(&root) {
            let p = lib.join("steamapps/common/Underkeep");
            if p.join("base").is_dir() {
                return Some(p);
            }
        }
    }
    gog_game_dirs()
        .into_iter()
        .find(|p| p.join("base").is_dir())
}

/// Does this folder look like it holds Underkeep saves?
pub fn looks_like_save_dir(p: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(p) else {
        return false;
    };
    rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_lowercase();
        n.starts_with("save_") || n == "player_profile"
    })
}

/// Stricter check for folders that might belong to another game (GOG Galaxy
/// keeps every game's cloud saves side by side): only Underkeep's own file names.
fn looks_like_underkeep_saves(p: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(p) else {
        return false;
    };
    rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_lowercase();
        n == "player_profile"
            || n == "save_auto"
            || n == "save_quick"
            || n.strip_prefix("save_")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Underkeep save folders under a GOG Galaxy `Applications` folder
/// (`%LOCALAPPDATA%\GOG.com\Galaxy\Applications`). Each game has a numeric
/// folder there, with its cloud-synced files in `Storage\Shared\Files`.
fn gog_save_dirs(applications: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(applications) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path().join("Storage/Shared/Files"))
        .filter(|p| looks_like_underkeep_saves(p))
        .collect();
    out.sort();
    out
}

/// GOG Galaxy `Applications` folders: the native one on Windows, plus one per
/// user inside each Wine prefix.
fn gog_application_roots() -> Vec<PathBuf> {
    const REL: &str = "GOG.com/Galaxy/Applications";
    let mut out = Vec::new();
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        out.push(local.join(REL));
    }
    for prefix in wine_prefixes() {
        if let Ok(rd) = std::fs::read_dir(prefix.join("drive_c/users")) {
            for user in rd.flatten() {
                out.push(user.path().join("AppData/Local").join(REL));
                // Windows XP-style layout used by some old prefixes.
                out.push(
                    user.path()
                        .join("Local Settings/Application Data")
                        .join(REL),
                );
            }
        }
    }
    out
}

/// All folders that look like they hold Underkeep saves.
pub fn find_save_dirs() -> Vec<SaveDir> {
    let mut out: Vec<SaveDir> = Vec::new();
    let mut add = |path: PathBuf, label: String| {
        if looks_like_save_dir(&path) && !out.iter().any(|s| s.path == path) {
            out.push(SaveDir { path, label });
        }
    };

    for root in steam_roots() {
        let in_bottle = root.ancestors().any(|a| a.ends_with("drive_c"));
        if let Ok(rd) = std::fs::read_dir(root.join("userdata")) {
            for user in rd.flatten() {
                let id = user.file_name().to_string_lossy().into_owned();
                let p = user.path().join(APP_ID).join("remote");
                let label = if in_bottle {
                    format!("Steam Cloud, user {id} (Wine bottle)")
                } else {
                    format!("Steam Cloud, user {id}")
                };
                add(p, label);
            }
        }
        // Local (non-cloud) saves inside a Proton prefix.
        for lib in library_folders(&root) {
            let docs = lib.join(format!(
                "steamapps/compatdata/{APP_ID}/pfx/drive_c/users/steamuser/Documents/Underkeep"
            ));
            add(docs, "Proton prefix (local saves)".into());
        }
    }

    // GOG Galaxy cloud saves.
    for apps in gog_application_roots() {
        let in_bottle = apps.ancestors().any(|a| a.ends_with("drive_c"));
        for p in gog_save_dirs(&apps) {
            let id = p
                .ancestors()
                .nth(3)
                .and_then(|a| a.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let label = if in_bottle {
                format!("GOG Galaxy, game {id} (Wine bottle)")
            } else {
                format!("GOG Galaxy, game {id}")
            };
            add(p, label);
        }
    }

    // Local (non-cloud) saves in Documents.
    if let Some(docs) = dirs::document_dir() {
        add(docs.join("Underkeep"), "Documents (local saves)".into());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_something_locally() {
        // Informational only: prints what the scanner sees on this machine.
        eprintln!("steam roots: {:?}", super::steam_roots());
        eprintln!("game dir: {:?}", super::find_game_dir());
        eprintln!("save dirs: {:?}", super::find_save_dirs());
    }

    #[test]
    fn finds_gog_saves_and_skips_other_games() {
        // Fixed folder, reused between runs (the files are rewritten, never removed).
        let tmp = std::env::temp_dir().join("underkeeper-gog-test");
        let apps = tmp.join("GOG.com/Galaxy/Applications");
        let uk = apps.join("60010286278343253/Storage/Shared/Files");
        let other = apps.join("12345/Storage/Shared/Files");
        std::fs::create_dir_all(&uk).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(uk.join("save_1"), b"x").unwrap();
        std::fs::write(other.join("save_slot_a.dat"), b"x").unwrap();
        let found = super::gog_save_dirs(&apps);
        assert_eq!(found, vec![uk]);
    }
}
