//! Exporting an encounter's monsters for Foundry VTT: one actor JSON file
//! (dnd5e) per monster, built by `encompile.foundry`.
//!
//! The rosters decide which files: a stat block used under no other name is
//! written once, with its own name; each name it's used under (a roster
//! entry's local display or short name, else its reference's) is a file of
//! its own, with the name set on the actor (its sheet) and on its prototype
//! token (the short name, if there is one). `scripts/foundry_export.py`,
//! embedded here and run through uv like the stat block script, does the
//! building and writing.

use crate::config::Config;
use crate::encounter::Encounter;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SCRIPT: &str = include_str!("../scripts/foundry_export.py");

/// One file to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Actor {
    /// The monster's file and its `id` there.
    pub file: PathBuf,
    pub monster: String,
    /// The actor's name; None keeps the stat block's.
    pub name: Option<String>,
    /// The prototype token's name; None keeps the actor's.
    pub token: Option<String>,
    /// The file name, `fvtt-Actor-<monster>[-<name>].json`.
    pub out: String,
}

impl Actor {
    /// How the export list shows it.
    pub fn label(&self) -> String {
        let name = self.name.clone().unwrap_or_else(|| self.monster.clone());
        match &self.token {
            Some(t) if Some(t) != self.name.as_ref() => format!("{name} (token: {t})"),
            _ => name,
        }
    }
}

/// What an export of an encounter will write, and what it can't.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub actors: Vec<Actor>,
    /// Roster monsters that can't be exported (no reference).
    pub problems: Vec<String>,
}

/// The files for every monster in every roster of the encounter.
pub fn plan(e: &Encounter, config: &Config) -> Plan {
    let mut plan = Plan::default();
    let rosters = e
        .groups
        .iter()
        .flat_map(|g| std::iter::once(&g.base_roster).chain(g.alt_rosters.iter().map(|a| &a.monsters)));
    for entry in rosters.flatten() {
        let Some(reference) = e.references.iter().find(|r| r.id == entry.id) else {
            let problem = format!("{} has no reference, so it isn't exported.", entry.id);
            if !plan.problems.contains(&problem) {
                plan.problems.push(problem);
            }
            continue;
        };
        let file = config.resolve(&reference.file);
        let name = entry.display_name.clone().or_else(|| reference.display_name.clone());
        let token = entry.short_name.clone().or_else(|| reference.short_name.clone());
        if plan
            .actors
            .iter()
            .any(|a| a.file == file && a.monster == reference.monster && a.name == name && a.token == token)
        {
            continue;
        }
        let stem = match name.as_ref().or(token.as_ref()) {
            Some(n) => format!("fvtt-Actor-{}-{}", reference.monster, slug(n)),
            None => format!("fvtt-Actor-{}", reference.monster),
        };
        let mut out = format!("{stem}.json");
        let mut n = 2;
        while plan.actors.iter().any(|a| a.out.eq_ignore_ascii_case(&out)) {
            out = format!("{stem}-{n}.json");
            n += 1;
        }
        plan.actors.push(Actor { file, monster: reference.monster.clone(), name, token, out });
    }
    plan
}

/// "Red Fang's Crew" -> "red-fang-s-crew".
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// An export to run.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    /// The folder to write to.
    pub out: PathBuf,
    /// The resource folder, for shorter paths in the compiler's messages.
    pub root: PathBuf,
    pub actors: Vec<Actor>,
    #[serde(skip)]
    pub compiler_dir: PathBuf,
    #[serde(skip)]
    pub uv_command: String,
    #[serde(skip)]
    pub cache_root: PathBuf,
}

/// What an export did.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Report {
    pub written: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// How exports are run.
#[derive(Clone, Default)]
pub enum Exporter {
    /// The real compiler, through uv.
    #[default]
    Compiler,
    /// A stand-in for tests.
    #[cfg(test)]
    Fake(std::sync::Arc<dyn Fn(&Job) -> Result<Report, String> + Send + Sync>),
}

impl std::fmt::Debug for Exporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Exporter::Compiler => f.write_str("Compiler"),
            #[cfg(test)]
            Exporter::Fake(_) => f.write_str("Fake"),
        }
    }
}

/// Writes the job's files, creating its folder first.
pub async fn export(exporter: Exporter, job: Job) -> Result<Report, String> {
    std::fs::create_dir_all(&job.out).map_err(|e| format!("Couldn't create {}: {e}", job.out.display()))?;
    match exporter {
        Exporter::Compiler => run_compiler(&job).await,
        #[cfg(test)]
        Exporter::Fake(f) => f(&job),
    }
}

async fn run_compiler(job: &Job) -> Result<Report, String> {
    std::fs::create_dir_all(&job.cache_root)
        .map_err(|e| format!("Couldn't create {}: {e}", job.cache_root.display()))?;
    let script = write_if_changed(&job.cache_root.join("foundry_export.py"), SCRIPT)?;
    let job_file = job.cache_root.join("foundry-job.json");
    let body = serde_json::to_string_pretty(job).map_err(|e| e.to_string())?;
    std::fs::write(&job_file, body).map_err(|e| format!("Couldn't write {}: {e}", job_file.display()))?;

    let output = crate::statblock::compiler_command(&job.compiler_dir, &job.uv_command, &job.cache_root)?
        .arg(&script)
        .arg(&job.compiler_dir)
        .arg(&job_file)
        .output()
        .await
        .map_err(|e| crate::statblock::uv_missing(&job.uv_command, e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    match stdout.lines().rev().find_map(|l| serde_json::from_str::<Report>(l).ok()) {
        Some(report) => Ok(report),
        None => Err(crate::statblock::failure(&output.stderr)),
    }
}

fn write_if_changed(path: &Path, text: &str) -> Result<PathBuf, String> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        std::fs::write(path, text).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter::{AltRoster, Group, Reference, RosterEntry};

    fn reference(id: &str, monster: &str, display: Option<&str>, short: Option<&str>) -> Reference {
        Reference {
            id: id.into(),
            file: "res://monsters/bandits.xml".into(),
            monster: monster.into(),
            display_name: display.map(Into::into),
            short_name: short.map(Into::into),
        }
    }

    fn named(id: &str, display: &str, short: Option<&str>) -> RosterEntry {
        RosterEntry { display_name: Some(display.into()), short_name: short.map(Into::into), ..RosterEntry::new(id, 1) }
    }

    #[test]
    fn a_stat_block_used_under_one_name_is_written_once() {
        let mut e = Encounter::default();
        e.references = vec![reference("sniper", "bandit-sniper", None, None)];
        e.groups[0].base_roster = vec![RosterEntry::new("sniper", 2)];
        e.groups[0].alt_rosters =
            vec![AltRoster { level: 2, num_pcs: 4, monsters: vec![RosterEntry::new("sniper", 3)] }];
        let mut second = Group::new("Second wave");
        second.base_roster = vec![RosterEntry::new("sniper", 1)];
        e.groups.push(second);

        let plan = plan(&e, &Config::default());
        assert!(plan.problems.is_empty());
        assert_eq!(
            plan.actors,
            vec![Actor {
                file: Config::default().resolve("res://monsters/bandits.xml"),
                monster: "bandit-sniper".into(),
                name: None,
                token: None,
                out: "fvtt-Actor-bandit-sniper.json".into(),
            }]
        );
    }

    #[test]
    fn each_name_a_stat_block_is_used_under_is_a_file() {
        let mut e = Encounter::default();
        e.references = vec![
            reference("captain", "bandit-captain", None, None),
            // Two references to one stat block, with their own names.
            reference("sniper", "bandit-deadeye", Some("Fyier Forest Sniper"), Some("Sniper")),
            reference("deadeye", "bandit-deadeye", Some("Fyier Forest Deadeye"), None),
        ];
        e.groups[0].base_roster = vec![
            named("captain", "Red Fang", Some("Fang")),
            named("captain", "Blue Fang", None),
            RosterEntry::new("captain", 1), // the plain stat block too
            RosterEntry::new("sniper", 2),
            RosterEntry::new("deadeye", 1),
        ];
        // The same names again in an alternate roster: no new files.
        e.groups[0].alt_rosters = vec![AltRoster {
            level: 2,
            num_pcs: 4,
            monsters: vec![named("captain", "Red Fang", Some("Fang")), RosterEntry::new("sniper", 3)],
        }];

        let plan = plan(&e, &Config::default());
        let got: Vec<_> =
            plan.actors.iter().map(|a| (a.out.as_str(), a.name.as_deref(), a.token.as_deref())).collect();
        assert_eq!(
            got,
            vec![
                ("fvtt-Actor-bandit-captain-red-fang.json", Some("Red Fang"), Some("Fang")),
                ("fvtt-Actor-bandit-captain-blue-fang.json", Some("Blue Fang"), None),
                ("fvtt-Actor-bandit-captain.json", None, None),
                ("fvtt-Actor-bandit-deadeye-fyier-forest-sniper.json", Some("Fyier Forest Sniper"), Some("Sniper")),
                ("fvtt-Actor-bandit-deadeye-fyier-forest-deadeye.json", Some("Fyier Forest Deadeye"), None),
            ]
        );
        assert_eq!(plan.actors[0].label(), "Red Fang (token: Fang)");
        assert_eq!(plan.actors[1].label(), "Blue Fang");
        assert_eq!(plan.actors[2].label(), "bandit-captain");
    }

    #[test]
    fn a_local_short_name_alone_names_the_token() {
        let mut e = Encounter::default();
        e.references = vec![reference("captain", "bandit-captain", Some("Captain"), None)];
        e.groups[0].base_roster =
            vec![RosterEntry { short_name: Some("Cap".into()), ..RosterEntry::new("captain", 1) }];
        let plan = plan(&e, &Config::default());
        // The reference's display name, the entry's short name.
        assert_eq!(plan.actors[0].name.as_deref(), Some("Captain"));
        assert_eq!(plan.actors[0].token.as_deref(), Some("Cap"));
    }

    #[test]
    fn file_names_never_clash() {
        let mut e = Encounter::default();
        // Same monster id in two files, and two names that slug the same.
        let mut other = reference("goblin2", "goblin", None, None);
        other.file = "res://srd/goblin.xml".into();
        e.references = vec![reference("goblin", "goblin", None, None), other];
        e.groups[0].base_roster = vec![
            RosterEntry::new("goblin", 1),
            RosterEntry::new("goblin2", 1),
            named("goblin", "Snik!", None),
            named("goblin", "Snik?", None),
        ];
        let outs: Vec<_> = plan(&e, &Config::default()).actors.into_iter().map(|a| a.out).collect();
        assert_eq!(
            outs,
            vec![
                "fvtt-Actor-goblin.json",
                "fvtt-Actor-goblin-2.json",
                "fvtt-Actor-goblin-snik.json",
                "fvtt-Actor-goblin-snik-2.json"
            ]
        );
    }

    #[test]
    fn monsters_without_a_reference_are_reported_once() {
        let mut e = Encounter::default();
        e.groups[0].base_roster = vec![RosterEntry::new("ghost", 1), named("ghost", "Boo", None)];
        let plan = plan(&e, &Config::default());
        assert!(plan.actors.is_empty());
        assert_eq!(plan.problems, vec!["ghost has no reference, so it isn't exported.".to_string()]);
    }

    /// The real compiler on the SRD goblin, renamed. Needs uv and the
    /// enigma-narrative repository; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_compiler_exports_a_renamed_goblin() {
        let config = Config::default();
        let out = tempfile::tempdir().unwrap();
        let file = config.resolve("res://shared/internal/monsters/goblin-warrior.xml");
        let actors = vec![
            Actor { file: file.clone(), monster: "goblin-warrior".into(), name: None, token: None, out: "plain.json".into() },
            Actor {
                file,
                monster: "goblin-warrior".into(),
                name: Some("Snik the Sneak".into()),
                token: Some("Snik".into()),
                out: "snik.json".into(),
            },
        ];
        let job = Job {
            out: out.path().to_path_buf(),
            root: config.resolve("res://"),
            actors,
            compiler_dir: config.resolve(&config.compiler_dir),
            uv_command: config.uv_command.clone(),
            cache_root: crate::statblock::default_cache_root(),
        };
        let report = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(export(Exporter::Compiler, job))
            .unwrap();
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(report.written.len(), 2, "{report:?}");
        let read = |name: &str| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(out.path().join(name)).unwrap()).unwrap()
        };
        let plain = read("plain.json");
        assert_eq!(plain["name"], "Goblin Warrior");
        assert_eq!(plain["prototypeToken"]["name"], "Goblin Warrior");
        let snik = read("snik.json");
        assert_eq!(snik["name"], "Snik the Sneak");
        assert_eq!(snik["prototypeToken"]["name"], "Snik");
        assert_eq!(snik["system"], plain["system"]); // the same stat block
    }
}
