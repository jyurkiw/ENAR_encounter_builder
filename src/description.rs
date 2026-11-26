//! Encounter descriptions: a book-style write-up of an encounter, with a
//! summary table per chosen roster (Name, Number, CR, AC, HP, XP) and a
//! bestiary of the chosen stat blocks, each under the short name it's used by.
//!
//! The description is a supplement compiled by encompile, written and run by
//! `scripts/encounter_description.py` (embedded here and run through uv like
//! the stat block script). It makes a PDF, which the description window shows
//! as page images and can save a copy of.

use crate::config::Config;
use crate::encounter::{Encounter, Reference, RosterRef, RosterSel};
use crate::xp::{self, Difficulty};
use serde::Serialize;
use std::path::{Path, PathBuf};

const SCRIPT: &str = include_str!("../scripts/encounter_description.py");
/// Page images: wide enough for the window, small enough to compile quickly.
const DPI: u32 = 110;

/// A roster that can go into the description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterOption {
    pub at: RosterRef,
    /// "Group 1 · base roster · level 3, 5 PCs".
    pub label: String,
}

/// Every roster in the encounter, group by group.
pub fn roster_options(e: &Encounter) -> Vec<RosterOption> {
    let mut out = Vec::new();
    for (gi, g) in e.groups.iter().enumerate() {
        let sels = std::iter::once(RosterSel::Base).chain((0..g.alt_rosters.len()).map(RosterSel::Alt));
        for roster in sels {
            let at = RosterRef { group: gi, roster };
            let (level, pcs) = e.party(at).unwrap_or((e.level, e.num_pcs));
            let kind = match roster {
                RosterSel::Base => "base roster",
                RosterSel::Alt(_) => "alternate roster",
            };
            out.push(RosterOption { at, label: format!("{} · {kind} · {}", g.name, party(level, pcs)) });
        }
    }
    out
}

fn party(level: u32, pcs: u32) -> String {
    format!("level {level}, {pcs} PC{}", if pcs == 1 { "" } else { "s" })
}

/// A stat block that can go into the bestiary: one per monster and name it's
/// used under in the chosen rosters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockOption {
    pub file: PathBuf,
    pub monster: String,
    /// The roster's (or reference's) display and short names for it.
    pub display: Option<String>,
    pub short: Option<String>,
    /// The stat block's own name.
    pub block_name: String,
    /// What prints above the stat block: the short name (else the display
    /// name), when it isn't the stat block's own name.
    pub heading: Option<String>,
}

impl BlockOption {
    /// How the modal lists it.
    pub fn label(&self) -> String {
        match &self.heading {
            Some(h) => format!("{h} ({})", self.block_name),
            None => self.block_name.clone(),
        }
    }

    fn same(&self, other: &BlockOption) -> bool {
        self.file == other.file
            && self.monster == other.monster
            && self.display == other.display
            && self.short == other.short
    }
}

/// What the monster files say about a reference's stat block: its name and
/// CR, from the monster browser's scan.
pub type Lookup<'a> = dyn Fn(&Reference) -> Option<(String, Option<String>)> + 'a;

/// The bestiary's stat blocks for these rosters, in roster order, and the
/// roster monsters that have no reference (so no stat block).
pub fn block_options(
    e: &Encounter,
    rosters: &[RosterRef],
    config: &Config,
    lookup: &Lookup<'_>,
) -> (Vec<BlockOption>, Vec<String>) {
    let (mut blocks, mut missing) = (Vec::<BlockOption>::new(), Vec::new());
    for entry in rosters.iter().filter_map(|&at| e.roster(at)).flatten() {
        let Some(r) = e.references.iter().find(|r| r.id == entry.id) else {
            if !missing.contains(&entry.id) {
                missing.push(entry.id.clone());
            }
            continue;
        };
        let block_name = lookup(r).map(|(name, _)| name).unwrap_or_else(|| r.monster.clone());
        let display = entry.display_name.clone().or_else(|| r.display_name.clone());
        let short = entry.short_name.clone().or_else(|| r.short_name.clone());
        let heading = short.clone().or_else(|| display.clone()).filter(|h| *h != block_name);
        let option = BlockOption { file: config.resolve(&r.file), monster: r.monster.clone(), display, short, block_name, heading };
        if !blocks.iter().any(|b| b.same(&option)) {
            blocks.push(option);
        }
    }
    (blocks, missing)
}

/// What the script compiles. See scripts/encounter_description.py.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Job {
    pub title: String,
    pub root: PathBuf,
    pub out: PathBuf,
    pub dpi: u32,
    pub monsters: Vec<JobMonster>,
    /// The chosen rosters, by the party they're for, fewest levels and PCs first.
    pub parties: Vec<JobParty>,
    pub bestiary: Vec<JobBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobMonster {
    #[serde(rename = "ref")]
    pub reference: String,
    pub file: PathBuf,
    pub monster: String,
}

/// Every chosen roster for one party (a level and a number of PCs).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobParty {
    /// "Level 3, 5 PCs".
    pub title: String,
    #[serde(skip)]
    pub level: u32,
    #[serde(skip)]
    pub pcs: u32,
    pub rosters: Vec<JobRoster>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobRoster {
    /// The group's name.
    pub group: String,
    /// "Moderate to High".
    pub difficulty: String,
    /// "Total 1,000 XP: High."
    pub summary: String,
    pub rows: Vec<JobRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobRow {
    /// The stat block's ref in `monsters`, for its AC and HP.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub name: String,
    pub count: u32,
    pub cr: Option<String>,
    pub xp: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobBlock {
    #[serde(rename = "ref")]
    pub reference: String,
    pub heading: Option<String>,
}

/// The description of these rosters and stat blocks. `out` is the folder to
/// compile in.
pub fn job(
    e: &Encounter,
    rosters: &[RosterRef],
    blocks: &[BlockOption],
    config: &Config,
    lookup: &Lookup<'_>,
    out: PathBuf,
) -> Job {
    let mut monsters: Vec<JobMonster> = Vec::new();
    let mut monster_ref = |file: PathBuf, monster: &str| -> String {
        if let Some(m) = monsters.iter().find(|m| m.file == file && m.monster == monster) {
            return m.reference.clone();
        }
        let reference = format!("m{}", monsters.len());
        monsters.push(JobMonster { reference: reference.clone(), file, monster: monster.to_string() });
        reference
    };

    let mut parties: Vec<JobParty> = Vec::new();
    for &at in rosters {
        let (Some(g), Some(entries)) = (e.groups.get(at.group), e.roster(at)) else { continue };
        let (level, pcs) = e.party(at).unwrap_or((e.level, e.num_pcs));
        let rows: Vec<JobRow> = entries
            .iter()
            .map(|entry| {
                let r = e.references.iter().find(|r| r.id == entry.id);
                let found = r.and_then(lookup);
                let block_name = found.as_ref().map(|(n, _)| n.clone());
                let cr = found.and_then(|(_, cr)| cr);
                let name = entry
                    .display_name
                    .clone()
                    .or_else(|| r.and_then(|r| r.display_name.clone()))
                    .or(block_name)
                    .unwrap_or_else(|| entry.id.clone());
                JobRow {
                    reference: r.map(|r| monster_ref(config.resolve(&r.file), &r.monster)),
                    name,
                    count: entry.count,
                    xp: cr.as_deref().and_then(xp::for_cr).map(|x| x * entry.count),
                    cr,
                }
            })
            .collect();
        let total: u32 = rows.iter().filter_map(|r| r.xp).sum();
        let unknown = rows.iter().any(|r| r.xp.is_none());
        let summary = match (total, xp::budget(level, pcs)) {
            (0, _) if !unknown => "No monsters.".to_string(),
            (t, Some(budget)) => format!(
                "Total {}{} XP: {}.",
                xp::thousands(t.into()),
                if unknown { "+" } else { "" },
                match Difficulty::of(t, budget) {
                    Difficulty::OverBudget => "over the High budget".to_string(),
                    d => d.label().to_string(),
                }
            ),
            (t, None) => format!("Total {} XP.", xp::thousands(t.into())),
        };
        let difficulty = if g.min_difficulty == g.max_difficulty {
            capitalised(g.min_difficulty.name())
        } else {
            format!("{} to {}", capitalised(g.min_difficulty.name()), capitalised(g.max_difficulty.name()))
        };
        let roster = JobRoster { group: g.name.clone(), difficulty, summary, rows };
        match parties.iter_mut().find(|p| p.level == level && p.pcs == pcs) {
            Some(p) => p.rosters.push(roster),
            None => parties.push(JobParty {
                title: capitalised(&party(level, pcs)),
                level,
                pcs,
                rosters: vec![roster],
            }),
        }
    }
    // Stable: within a party, the rosters stay in the encounter's group order.
    parties.sort_by_key(|p| (p.level, p.pcs));

    let bestiary = blocks
        .iter()
        .map(|b| JobBlock { reference: monster_ref(b.file.clone(), &b.monster), heading: b.heading.clone() })
        .collect();
    Job {
        title: title(&e.id),
        root: PathBuf::from(&config.resource_dir),
        out,
        dpi: DPI,
        monsters,
        parties,
        bestiary,
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// "bridge-ambush" -> "Bridge Ambush".
pub fn title(id: &str) -> String {
    id.split(['-', '_', ' ']).filter(|w| !w.is_empty()).map(capitalised).collect::<Vec<_>>().join(" ")
}

/// Where an encounter's description is compiled.
pub fn out_dir(cache_root: &Path, encounter_id: &str) -> PathBuf {
    let clean: String =
        encounter_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    cache_root.join("descriptions").join(if clean.is_empty() { "encounter".into() } else { clean })
}

/// A compiled description.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub pdf: PathBuf,
    pub pages: Vec<Vec<u8>>,
    pub warnings: Vec<String>,
}

/// How descriptions are compiled.
#[derive(Clone, Default)]
pub enum Compiler {
    /// encompile, through uv.
    #[default]
    Encompile,
    /// A stand-in for tests: called with the job, it writes the PDF and the
    /// page images into `job.out` and returns the warnings.
    #[cfg(test)]
    Fake(std::sync::Arc<FakeCompile>),
}

#[cfg(test)]
pub type FakeCompile = dyn Fn(&Job) -> Result<Vec<String>, String> + Send + Sync;

impl std::fmt::Debug for Compiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Compiler::Encompile => f.write_str("Encompile"),
            #[cfg(test)]
            Compiler::Fake(_) => f.write_str("Fake"),
        }
    }
}

/// Everything a compile needs besides the job, gathered on the UI side.
#[derive(Debug, Clone)]
pub struct Tools {
    pub compiler_dir: PathBuf,
    pub uv_command: String,
    pub cache_root: PathBuf,
}

/// Compiles the description and reads its pages.
pub async fn compile(compiler: Compiler, job: Job, tools: Tools) -> Result<Compiled, String> {
    std::fs::create_dir_all(&job.out).map_err(|e| format!("Couldn't create {}: {e}", job.out.display()))?;
    let warnings = match compiler {
        Compiler::Encompile => run_compiler(&job, &tools).await?,
        #[cfg(test)]
        Compiler::Fake(f) => f(&job)?,
    };
    let pdf = job.out.join("description.pdf");
    if !pdf.is_file() {
        return Err("The compiler made no PDF".into());
    }
    let pages = crate::statblock::load(&crate::statblock::pages(&job.out))?;
    if pages.is_empty() {
        return Err("The compiler made no page images".into());
    }
    Ok(Compiled { pdf, pages, warnings })
}

async fn run_compiler(job: &Job, tools: &Tools) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(&tools.cache_root)
        .map_err(|e| format!("Couldn't create {}: {e}", tools.cache_root.display()))?;
    let script = tools.cache_root.join("encounter_description.py");
    if std::fs::read_to_string(&script).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&script, SCRIPT).map_err(|e| format!("Couldn't write {}: {e}", script.display()))?;
    }
    let job_file = tools.cache_root.join("description-job.json");
    let body = serde_json::to_string_pretty(job).map_err(|e| e.to_string())?;
    std::fs::write(&job_file, body).map_err(|e| format!("Couldn't write {}: {e}", job_file.display()))?;

    let output = crate::statblock::compiler_command(&tools.compiler_dir, &tools.uv_command, &tools.cache_root)?
        .arg(&script)
        .arg(&tools.compiler_dir)
        .arg(&job_file)
        .output()
        .await
        .map_err(|e| crate::statblock::uv_missing(&tools.uv_command, e))?;
    if !output.status.success() {
        return Err(crate::statblock::failure(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("warning: ").map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter::{AltRoster, Group, RosterEntry};

    fn reference(id: &str, monster: &str, display: Option<&str>, short: Option<&str>) -> Reference {
        Reference {
            id: id.into(),
            file: format!("C:/m/{monster}.xml"),
            monster: monster.into(),
            display_name: display.map(Into::into),
            short_name: short.map(Into::into),
        }
    }

    fn named(id: &str, count: u32, display: Option<&str>, short: Option<&str>) -> RosterEntry {
        RosterEntry { display_name: display.map(Into::into), short_name: short.map(Into::into), ..RosterEntry::new(id, count) }
    }

    /// A base roster of 2 snipers and Dohovos; a level 4 roster adding Remors.
    fn encounter() -> Encounter {
        let mut g = Group::new("Bridge");
        g.max_difficulty = crate::xp::Tier::High;
        g.base_roster = vec![named("sniper", 2, None, None), named("triton", 1, Some("Dohovos Threll"), Some("Dohovos"))];
        g.alt_rosters = vec![AltRoster {
            level: 4,
            num_pcs: 5,
            monsters: vec![
                named("triton", 1, Some("Dohovos Threll"), Some("Dohovos")),
                named("triton", 1, Some("Remors Threll"), Some("Remors")),
                named("ghost", 1, None, None),
            ],
        }];
        Encounter {
            id: "bridge-ambush".into(),
            level: 3,
            num_pcs: 5,
            groups: vec![g],
            references: vec![
                reference("sniper", "bandit-sniper", Some("Fyier Forest Sniper"), Some("Sniper")),
                reference("triton", "triton_ambusher", None, None),
            ],
        }
    }

    fn lookup(r: &Reference) -> Option<(String, Option<String>)> {
        match r.monster.as_str() {
            "bandit-sniper" => Some(("Bandit Sniper".into(), Some("1/2".into()))),
            "triton_ambusher" => Some(("Triton Ambusher".into(), Some("1".into()))),
            _ => None,
        }
    }

    const BASE: RosterRef = RosterRef::FIRST;
    const ALT: RosterRef = RosterRef { group: 0, roster: RosterSel::Alt(0) };

    #[test]
    fn every_roster_is_an_option() {
        let labels: Vec<_> = roster_options(&encounter()).into_iter().map(|o| o.label).collect();
        assert_eq!(labels, ["Bridge · base roster · level 3, 5 PCs", "Bridge · alternate roster · level 4, 5 PCs"]);
    }

    #[test]
    fn a_stat_block_is_listed_once_per_name_with_its_short_name_on_top() {
        let (blocks, missing) = block_options(&encounter(), &[BASE, ALT], &Config::default(), &lookup);
        let labels: Vec<_> = blocks.iter().map(BlockOption::label).collect();
        assert_eq!(labels, ["Sniper (Bandit Sniper)", "Dohovos (Triton Ambusher)", "Remors (Triton Ambusher)"]);
        assert_eq!(missing, ["ghost"]);
    }

    #[test]
    fn only_the_chosen_rosters_give_stat_blocks() {
        let (blocks, missing) = block_options(&encounter(), &[BASE], &Config::default(), &lookup);
        assert_eq!(blocks.len(), 2);
        assert!(missing.is_empty());
    }

    #[test]
    fn a_name_the_same_as_the_stat_blocks_prints_no_heading() {
        let mut e = encounter();
        e.references[0].short_name = Some("Bandit Sniper".into());
        let (blocks, _) = block_options(&e, &[BASE], &Config::default(), &lookup);
        assert_eq!(blocks[0].heading, None);
        assert_eq!(blocks[0].label(), "Bandit Sniper");
    }

    #[test]
    fn the_job_has_a_table_per_roster_and_the_chosen_blocks() {
        let e = encounter();
        let config = Config::default();
        let (blocks, _) = block_options(&e, &[BASE, ALT], &config, &lookup);
        let job = job(&e, &[BASE, ALT], &blocks[1..], &config, &lookup, PathBuf::from("C:/out"));
        assert_eq!(job.title, "Bridge Ambush");
        let titles: Vec<_> = job.parties.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Level 3, 5 PCs", "Level 4, 5 PCs"]);
        let base = &job.parties[0].rosters[0];
        assert_eq!((base.group.as_str(), base.difficulty.as_str()), ("Bridge", "Moderate to High"));
        let rows: Vec<_> = base.rows.iter().map(|r| (r.name.as_str(), r.count, r.xp)).collect();
        assert_eq!(rows, [("Fyier Forest Sniper", 2, Some(200)), ("Dohovos Threll", 1, Some(200))]);
        assert_eq!(base.summary, "Total 400 XP: Low.");
        // The ghost has no reference: no stat block, no XP.
        let alt = &job.parties[1].rosters[0];
        assert_eq!(alt.rows[2].reference, None);
        assert_eq!(alt.summary, "Total 400+ XP: Low.");
        // One monster entry per stat block, shared by the tables and the bestiary.
        assert_eq!(job.monsters.len(), 2);
        let headings: Vec<_> = job.bestiary.iter().map(|b| (b.reference.as_str(), b.heading.as_deref())).collect();
        assert_eq!(headings, [("m1", Some("Dohovos")), ("m1", Some("Remors"))]);
    }

    #[test]
    fn rosters_for_the_same_party_share_a_section_in_level_and_pc_order() {
        let mut e = encounter();
        // Group 2: a base for the encounter's party (level 3, 5 PCs), then
        // level 4 for 4 PCs and level 4 for 5 PCs.
        let mut g2 = Group::new("Pool");
        g2.base_roster = vec![named("sniper", 1, None, None)];
        let alt = |level, num_pcs| AltRoster { level, num_pcs, monsters: vec![named("sniper", 3, None, None)] };
        g2.alt_rosters = vec![alt(4, 4), alt(4, 5)];
        e.groups.push(g2);
        let at = |group, roster| RosterRef { group, roster };
        let chosen = [
            at(0, RosterSel::Base),
            at(0, RosterSel::Alt(0)),
            at(1, RosterSel::Base),
            at(1, RosterSel::Alt(0)),
            at(1, RosterSel::Alt(1)),
        ];
        let job = job(&e, &chosen, &[], &Config::default(), &lookup, PathBuf::from("C:/out"));
        let layout: Vec<(&str, Vec<&str>)> = job
            .parties
            .iter()
            .map(|p| (p.title.as_str(), p.rosters.iter().map(|r| r.group.as_str()).collect()))
            .collect();
        assert_eq!(
            layout,
            [
                ("Level 3, 5 PCs", vec!["Bridge", "Pool"]),
                ("Level 4, 4 PCs", vec!["Pool"]),
                ("Level 4, 5 PCs", vec!["Bridge", "Pool"]),
            ]
        );
    }

    #[test]
    fn titles_read_like_names() {
        assert_eq!(title("bridge-ambush"), "Bridge Ambush");
        assert_eq!(title("the_old_mill 2"), "The Old Mill 2");
    }

    /// The real compiler, on the SRD goblin warrior. Needs uv, LaTeX and the
    /// enigma-narrative repository; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_compiler_writes_a_description() {
        let config = Config::default();
        let mut g = Group::new("Goblins");
        g.base_roster = vec![named("gob", 3, None, None), named("gob", 1, Some("Snik the Sneak"), Some("Snik"))];
        let e = Encounter {
            id: "goblin-test".into(),
            level: 1,
            num_pcs: 4,
            groups: vec![g],
            references: vec![Reference {
                id: "gob".into(),
                file: "res://shared/internal/monsters/goblin-warrior.xml".into(),
                monster: "goblin-warrior".into(),
                display_name: None,
                short_name: None,
            }],
        };
        let look = |_: &Reference| Some(("Goblin Warrior".to_string(), Some("1/4".to_string())));
        let (blocks, _) = block_options(&e, &[BASE], &config, &look);
        let out = tempfile::tempdir().unwrap();
        let job = job(&e, &[BASE], &blocks, &config, &look, out.path().join("desc"));
        let tools = Tools {
            compiler_dir: config.resolve(&config.compiler_dir),
            uv_command: "uv".into(),
            cache_root: crate::statblock::default_cache_root(),
        };
        let compiled = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(compile(Compiler::Encompile, job, tools))
            .unwrap();
        assert!(compiled.pdf.is_file());
        assert!(!compiled.pages.is_empty());
    }
}
