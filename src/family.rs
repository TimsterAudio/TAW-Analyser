//! Starting the rest of the family (Library, Player, Equaliser) from the
//! Analyser. None of them is needed: if one isn't found, the Analyser says
//! so and carries on. The same search as the Player's.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Library,
    Player,
    Equaliser,
    Analyser,
}

impl App {
    pub const ALL: [App; 4] = [App::Library, App::Player, App::Equaliser, App::Analyser];

    pub fn name(self) -> &'static str {
        match self {
            App::Library => "Library",
            App::Player => "Player",
            App::Equaliser => "Equaliser",
            App::Analyser => "Analyser",
        }
    }

    /// The program's file name, without ".exe".
    pub fn program(self) -> &'static str {
        match self {
            App::Library => "taw-library",
            App::Player => "taw-player",
            App::Equaliser => "taw-equaliser",
            App::Analyser => "taw-analyser",
        }
    }
}

pub fn exe_name(program: &str) -> String {
    if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    }
}

/// Where to look for `program`, in order: next to this program; in a
/// sibling folder named after it (two downloaded zips side by side); the
/// family's install folders; then PATH.
pub fn candidates(program: &str, here: Option<&Path>, path_var: Option<&str>) -> Vec<PathBuf> {
    let exe = exe_name(program);
    let mut v = Vec::new();
    if let Some(dir) = here {
        v.push(dir.join(&exe));
        if let Some(parent) = dir.parent() {
            if let Ok(rd) = std::fs::read_dir(parent) {
                let mut sib: Vec<PathBuf> = rd
                    .flatten()
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .starts_with(program)
                    })
                    .map(|e| e.path().join(&exe))
                    .collect();
                // Newest version last in name order, so try it first.
                sib.sort();
                sib.reverse();
                v.extend(sib);
            }
        }
    }
    if cfg!(windows) {
        let app = match program {
            "taw-library" => "Library",
            "taw-player" => "Player",
            "taw-equaliser" => "Equaliser",
            "taw-analyser" => "Analyser",
            _ => "",
        };
        for var in ["ProgramFiles", "LOCALAPPDATA"] {
            if let Ok(base) = std::env::var(var) {
                let base = PathBuf::from(base);
                let base = if var == "LOCALAPPDATA" {
                    base.join("Programs")
                } else {
                    base
                };
                v.push(base.join("Timster's Audio Works").join(app).join(&exe));
            }
        }
    }
    if let Some(path) = path_var {
        for dir in std::env::split_paths(path) {
            v.push(dir.join(&exe));
        }
    }
    v
}

/// The program, if it can be found. `remembered` is where the user last
/// pointed the Player to it.
pub fn find(program: &str, remembered: Option<&PathBuf>) -> Option<PathBuf> {
    if let Some(p) = remembered.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let here = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let path = std::env::var("PATH").ok();
    candidates(program, here.as_deref(), path.as_deref())
        .into_iter()
        .find(|p| p.is_file())
}

/// Starts a program on its own; the Player doesn't wait for it.
pub fn launch(exe: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_beside_then_siblings_then_path() {
        let d = std::env::temp_dir().join(format!("taw-analyser-family-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let here = d.join("taw-analyser-0.2.0");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::create_dir_all(d.join("taw-player-0.1.0")).unwrap();
        std::fs::create_dir_all(d.join("taw-player-0.2.0")).unwrap();
        let path = std::env::join_paths([d.join("bin")]).unwrap();
        let c = candidates("taw-player", Some(&here), path.to_str());
        let exe = exe_name("taw-player");
        assert_eq!(c[0], here.join(&exe));
        assert_eq!(c[1], d.join("taw-player-0.2.0").join(&exe));
        assert_eq!(c[2], d.join("taw-player-0.1.0").join(&exe));
        assert_eq!(c.last().unwrap(), &d.join("bin").join(&exe));
        let _ = std::fs::remove_dir_all(&d);
    }
}
