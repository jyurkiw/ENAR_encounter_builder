//! The designer's configuration file: where `res://` points and which folders
//! hold monster files.
//!
//! The file lives at `<config dir>/encounter-designer/config.toml`
//! (`%APPDATA%\encounter-designer\config.toml` on Windows). A missing file is
//! written with the defaults on first run.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RES_PREFIX: &str = "res://";
const DEFAULT_RESOURCE_DIR: &str = "e:/Data/enigma-narrative/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The folder `res://` paths resolve against.
    pub resource_dir: String,
    /// The folder of SRD monster files. Empty means none.
    pub srd_monster_dir: MonsterDir,
    /// Further folders of monster files.
    pub monster_dirs: Vec<MonsterDir>,
    /// The encompile compiler's folder, used to draw stat blocks.
    pub compiler_dir: String,
    /// How to run uv (a name on PATH or a full path).
    pub uv_command: String,
    /// The folder the last Foundry export went to.
    pub foundry_export_dir: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MonsterDir {
    /// An absolute path or a `res://` path.
    pub path: String,
    /// Also list monster files in folders below this one.
    pub include_subdirs: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resource_dir: DEFAULT_RESOURCE_DIR.to_string(),
            srd_monster_dir: MonsterDir::default(),
            monster_dirs: vec![MonsterDir {
                path: format!("{RES_PREFIX}monsters"),
                include_subdirs: true,
            }],
            compiler_dir: format!("{RES_PREFIX}compiler"),
            uv_command: "uv".to_string(),
            foundry_export_dir: String::new(),
        }
    }
}

impl MonsterDir {
    /// The path as the config list shows it: a trailing `*` marks a folder
    /// whose subfolders are included.
    pub fn display(&self) -> String {
        if self.include_subdirs {
            format!("{}*", self.path)
        } else {
            self.path.clone()
        }
    }
}

impl Config {
    /// Where the config file lives.
    pub fn file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("encounter-designer")
            .join("config.toml")
    }

    /// Loads the config file at `path`, writing the defaults if there is
    /// none. A file missing settings added since it was written gets them,
    /// with their defaults, so every setting can be found in the file. A
    /// file that can't be read or parsed gives the defaults and an error,
    /// and is left as it is.
    pub fn load_from(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(config) => {
                    let error = if missing_settings(&text) { config.save_to(path).err() } else { None };
                    (config, error)
                }
                Err(e) => (
                    Config::default(),
                    Some(format!("Couldn't parse {}: {e}. Using defaults.", path.display())),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                let error = config.save_to(path).err();
                (config, error)
            }
            Err(e) => (
                Config::default(),
                Some(format!("Couldn't read {}: {e}. Using defaults.", path.display())),
            ),
        }
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let text = format!(
            "# Encounter Designer settings.\n\
             # Paths may be absolute or start with res://, which resolves against resource_dir.\n\n{body}"
        );
        std::fs::write(path, text).map_err(|e| format!("Couldn't write {}: {e}", path.display()))
    }

    /// Turns a `res://` path into a real one; other paths pass through.
    pub fn resolve(&self, path: &str) -> PathBuf {
        match path.strip_prefix(RES_PREFIX) {
            Some(rest) => Path::new(&self.resource_dir).join(rest.trim_start_matches(['/', '\\'])),
            None => PathBuf::from(path),
        }
    }

    /// Turns a real path into a `res://` one when it lies inside the resource
    /// folder; otherwise gives it back unchanged.
    pub fn to_res_path(&self, path: &Path) -> String {
        let normalize = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
        let root = normalize(&self.resource_dir);
        let full = path.to_string_lossy().replace('\\', "/");
        if !root.is_empty() && full.to_lowercase().starts_with(&format!("{root}/")) {
            format!("{RES_PREFIX}{}", &full[root.len() + 1..])
        } else {
            full
        }
    }
}

/// Whether a config file lacks any top-level setting the program knows.
fn missing_settings(text: &str) -> bool {
    let Ok(file) = text.parse::<toml::Table>() else {
        return false;
    };
    let Ok(toml::Value::Table(all)) = toml::Value::try_from(Config::default()) else {
        return false;
    };
    all.keys().any(|k| !file.contains_key(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_an_older_file_adds_the_new_settings_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "resource_dir = \"d:/data/\"\n\n[[monster_dirs]]\npath = \"res://mine\"\ninclude_subdirs = true\n",
        )
        .unwrap();
        let (config, error) = Config::load_from(&path);
        assert!(error.is_none());
        assert_eq!(config.resource_dir, "d:/data/"); // kept
        assert_eq!(config.monster_dirs[0].path, "res://mine"); // kept
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("compiler_dir = \"res://compiler\""), "{text}");
        assert!(text.contains("uv_command = \"uv\""), "{text}");
        assert!(text.contains("[srd_monster_dir]"), "{text}");

        // A complete file is left exactly as it is.
        std::fs::write(&path, text.replace("# Encounter", "# edited by hand\n# Encounter")).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        Config::load_from(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn res_paths_round_trip() {
        let config = Config::default();
        let real = config.resolve("res://monsters/hedge_warden.xml");
        assert_eq!(real, Path::new("e:/Data/enigma-narrative/").join("monsters/hedge_warden.xml"));
        assert_eq!(
            config.to_res_path(Path::new(r"E:\Data\enigma-narrative\monsters")),
            "res://monsters"
        );
        assert_eq!(config.to_res_path(Path::new(r"C:\elsewhere")), "C:/elsewhere");
    }

    #[test]
    fn toml_round_trip() {
        let config = Config::default();
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }
}
