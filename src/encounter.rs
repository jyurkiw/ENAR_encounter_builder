//! The encounter file model (see `schemas/encounter_schema.xsd`), read from
//! and written to XML.

use crate::xp::Tier;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub struct Encounter {
    pub id: String,
    pub level: u32,
    pub num_pcs: u32,
    pub groups: Vec<Group>,
    pub references: Vec<Reference>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    /// The difficulty range the group aims for, for each of its rosters' parties.
    pub min_difficulty: Tier,
    pub max_difficulty: Tier,
    pub base_roster: Vec<RosterEntry>,
    pub alt_rosters: Vec<AltRoster>,
}

impl Group {
    /// An empty group aiming for Moderate.
    pub fn new(name: impl Into<String>) -> Group {
        Group {
            name: name.into(),
            min_difficulty: Tier::Moderate,
            max_difficulty: Tier::Moderate,
            base_roster: Vec::new(),
            alt_rosters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AltRoster {
    pub level: u32,
    pub num_pcs: u32,
    pub monsters: Vec<RosterEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RosterEntry {
    pub id: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub id: String,
    pub file: String,
    pub monster: String,
    pub display_name: Option<String>,
    pub short_name: Option<String>,
}

impl Default for Encounter {
    fn default() -> Self {
        Self {
            id: "new-encounter".into(),
            level: 1,
            num_pcs: 4,
            groups: vec![Group::new("Group 1")],
            references: Vec::new(),
        }
    }
}

impl Encounter {
    pub fn parse(text: &str) -> Result<Encounter, String> {
        let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        if root.tag_name().name() != "encounter" {
            return Err(format!("root element is <{}>, not <encounter>", root.tag_name().name()));
        }

        // Files from before the range moved onto groups set it on the
        // encounter: that's the default for groups without their own.
        let (min_default, max_default) = (tier(root, "minDifficulty")?, tier(root, "maxDifficulty")?);
        let groups = elements(root, "group")
            .map(|g| {
                Ok(Group {
                    name: g.attribute("name").unwrap_or_default().to_string(),
                    min_difficulty: tier_or(g, "minDifficulty", min_default)?,
                    max_difficulty: tier_or(g, "maxDifficulty", max_default)?,
                    base_roster: match elements(g, "baseMonsterRoster").next() {
                        Some(r) => roster(r)?,
                        None => Vec::new(),
                    },
                    alt_rosters: elements(g, "altMonsterRoster")
                        .map(|r| {
                            Ok(AltRoster {
                                level: number(r, "level", None)?,
                                num_pcs: number(r, "numPCs", None)?,
                                monsters: roster(r)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                })
            })
            .collect::<Result<_, String>>()?;

        let references = elements(root, "reference")
            .flat_map(|r| elements(r, "monster"))
            .map(|m| Reference {
                id: m.attribute("id").unwrap_or_default().to_string(),
                file: m.attribute("file").unwrap_or_default().to_string(),
                monster: m.attribute("monster").unwrap_or_default().to_string(),
                display_name: m.attribute("displayName").map(str::to_string),
                short_name: m.attribute("shortName").map(str::to_string),
            })
            .collect();

        Ok(Encounter {
            id: root.attribute("id").unwrap_or_default().to_string(),
            level: number(root, "level", None)?,
            num_pcs: number(root, "numPCs", None)?,
            groups,
            references,
        })
    }

    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        let _ = writeln!(
            s,
            r#"<encounter id="{}" level="{}" numPCs="{}">"#,
            esc(&self.id),
            self.level,
            self.num_pcs
        );
        for g in &self.groups {
            let _ = writeln!(
                s,
                r#"    <group name="{}" minDifficulty="{}" maxDifficulty="{}">"#,
                esc(&g.name),
                g.min_difficulty.name(),
                g.max_difficulty.name()
            );
            write_roster(&mut s, "baseMonsterRoster", "", &g.base_roster);
            for alt in &g.alt_rosters {
                let attrs = format!(r#" level="{}" numPCs="{}""#, alt.level, alt.num_pcs);
                write_roster(&mut s, "altMonsterRoster", &attrs, &alt.monsters);
            }
            let _ = writeln!(s, "    </group>");
        }
        let _ = writeln!(s, "    <reference>");
        for r in &self.references {
            let mut line = format!(
                r#"        <monster id="{}" file="{}" monster="{}""#,
                esc(&r.id),
                esc(&r.file),
                esc(&r.monster)
            );
            if let Some(v) = &r.display_name {
                let _ = write!(line, r#" displayName="{}""#, esc(v));
            }
            if let Some(v) = &r.short_name {
                let _ = write!(line, r#" shortName="{}""#, esc(v));
            }
            let _ = writeln!(s, "{line}/>");
        }
        let _ = writeln!(s, "    </reference>");
        let _ = writeln!(s, "</encounter>");
        s
    }
}

// ----- editing -----------------------------------------------------------------

/// Which roster of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterSel {
    Base,
    Alt(usize),
}

/// A roster in the encounter: its group and which of the group's rosters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterRef {
    pub group: usize,
    pub roster: RosterSel,
}

impl RosterRef {
    pub const FIRST: RosterRef = RosterRef { group: 0, roster: RosterSel::Base };
}

impl Encounter {
    pub fn roster(&self, at: RosterRef) -> Option<&Vec<RosterEntry>> {
        let g = self.groups.get(at.group)?;
        match at.roster {
            RosterSel::Base => Some(&g.base_roster),
            RosterSel::Alt(i) => g.alt_rosters.get(i).map(|a| &a.monsters),
        }
    }

    pub fn roster_mut(&mut self, at: RosterRef) -> Option<&mut Vec<RosterEntry>> {
        let g = self.groups.get_mut(at.group)?;
        match at.roster {
            RosterSel::Base => Some(&mut g.base_roster),
            RosterSel::Alt(i) => g.alt_rosters.get_mut(i).map(|a| &mut a.monsters),
        }
    }

    /// The party a roster is built for: the encounter's for a base roster.
    pub fn party(&self, at: RosterRef) -> Option<(u32, u32)> {
        match at.roster {
            RosterSel::Base => self.groups.get(at.group).map(|_| (self.level, self.num_pcs)),
            RosterSel::Alt(i) => {
                let a = self.groups.get(at.group)?.alt_rosters.get(i)?;
                Some((a.level, a.num_pcs))
            }
        }
    }

    /// Adds one of a monster to a roster, creating its reference if the
    /// encounter has none for that file and monster yet. Returns the
    /// reference id.
    pub fn add_monster(&mut self, at: RosterRef, file: &str, monster: &str) -> Option<String> {
        self.roster(at)?;
        let id = match self.references.iter().find(|r| r.file == file && r.monster == monster) {
            Some(r) => r.id.clone(),
            None => {
                let id = self.free_reference_id(monster);
                self.references.push(Reference {
                    id: id.clone(),
                    file: file.to_string(),
                    monster: monster.to_string(),
                    display_name: None,
                    short_name: None,
                });
                id
            }
        };
        let roster = self.roster_mut(at)?;
        match roster.iter_mut().find(|e| e.id == id) {
            Some(e) => e.count += 1,
            None => roster.push(RosterEntry { id: id.clone(), count: 1 }),
        }
        Some(id)
    }

    /// `base`, or `base-2`, `base-3`, ... if it's taken.
    fn free_reference_id(&self, base: &str) -> String {
        let taken = |id: &str| self.references.iter().any(|r| r.id == id);
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap()
    }

    /// A new alternate roster for a group: a copy of its base roster, for
    /// the next level up from its highest roster.
    pub fn add_alt_roster(&mut self, group: usize) -> Option<RosterRef> {
        let (level, num_pcs) = (self.level, self.num_pcs);
        let g = self.groups.get_mut(group)?;
        let top = g.alt_rosters.iter().map(|a| a.level).chain([level]).max().unwrap_or(level);
        g.alt_rosters.push(AltRoster {
            level: (top + 1).min(20),
            num_pcs,
            monsters: g.base_roster.clone(),
        });
        Some(RosterRef { group, roster: RosterSel::Alt(g.alt_rosters.len() - 1) })
    }

    pub fn add_group(&mut self) -> RosterRef {
        let n = self.groups.len() + 1;
        let name = (n..).map(|i| format!("Group {i}")).find(|name| !self.groups.iter().any(|g| &g.name == name)).unwrap();
        self.groups.push(Group::new(name));
        RosterRef { group: self.groups.len() - 1, roster: RosterSel::Base }
    }

    /// References no roster uses.
    pub fn unused_references(&self) -> Vec<&str> {
        self.references
            .iter()
            .filter(|r| {
                !self.groups.iter().any(|g| {
                    g.base_roster.iter().chain(g.alt_rosters.iter().flat_map(|a| &a.monsters)).any(|e| e.id == r.id)
                })
            })
            .map(|r| r.id.as_str())
            .collect()
    }

    /// Problems the schema would reject, in words, for the editor to show.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.id.trim().is_empty() {
            out.push("The encounter needs an id.".into());
        } else if !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            out.push("The encounter id can only use letters, digits, - and _.".into());
        }
        if self.groups.is_empty() {
            out.push("The encounter needs at least one group.".into());
        }
        for g in &self.groups {
            if g.base_roster.is_empty() {
                out.push(format!("{}: the base roster has no monsters.", g.name));
            }
            for (i, a) in g.alt_rosters.iter().enumerate() {
                if a.monsters.is_empty() {
                    out.push(format!("{}: the level {} roster for {} PCs has no monsters.", g.name, a.level, a.num_pcs));
                }
                if g.alt_rosters[..i].iter().any(|b| b.level == a.level && b.num_pcs == a.num_pcs) {
                    out.push(format!(
                        "{}: two alternate rosters are for level {} and {} PCs.",
                        g.name, a.level, a.num_pcs
                    ));
                }
            }
        }
        if self.references.is_empty() {
            out.push("The encounter has no monsters yet.".into());
        }
        out
    }
}

fn elements<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    tag: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    node.children().filter(move |c| c.has_tag_name(tag))
}

fn number(node: roxmltree::Node, attr: &str, default: Option<u32>) -> Result<u32, String> {
    match node.attribute(attr) {
        Some(v) => v.trim().parse().map_err(|_| {
            format!("<{}> has {attr}=\"{v}\", which isn't a whole number", node.tag_name().name())
        }),
        None => default.ok_or_else(|| format!("<{}> is missing {attr}", node.tag_name().name())),
    }
}

/// A difficulty attribute; Moderate when it's left out.
fn tier(node: roxmltree::Node, attr: &str) -> Result<Tier, String> {
    tier_or(node, attr, Tier::Moderate)
}

/// A difficulty attribute, or `default` when it's left out.
fn tier_or(node: roxmltree::Node, attr: &str, default: Tier) -> Result<Tier, String> {
    match node.attribute(attr) {
        None => Ok(default),
        Some(v) => Tier::parse(v).ok_or_else(|| {
            format!("<{}> has {attr}=\"{v}\"; use low, moderate or high", node.tag_name().name())
        }),
    }
}

fn roster(node: roxmltree::Node) -> Result<Vec<RosterEntry>, String> {
    elements(node, "monster")
        .map(|m| {
            Ok(RosterEntry {
                id: m.attribute("id").unwrap_or_default().to_string(),
                count: number(m, "count", Some(1))?,
            })
        })
        .collect()
}

fn write_roster(s: &mut String, tag: &str, attrs: &str, monsters: &[RosterEntry]) {
    let _ = writeln!(s, "        <{tag}{attrs}>");
    for m in monsters {
        if m.count == 1 {
            let _ = writeln!(s, r#"            <monster id="{}"/>"#, esc(&m.id));
        } else {
            let _ = writeln!(s, r#"            <monster id="{}" count="{}"/>"#, esc(&m.id), m.count);
        }
    }
    let _ = writeln!(s, "        </{tag}>");
}

fn esc(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_round_trips() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/encounters.xml")).unwrap();
        let encounter = Encounter::parse(&text).unwrap();
        assert_eq!(encounter.groups[0].alt_rosters.len(), 2);
        assert_eq!(encounter.groups[0].base_roster[4].count, 1);
        assert_eq!(Encounter::parse(&encounter.to_xml()).unwrap(), encounter);
    }

    #[test]
    fn each_group_has_its_own_difficulty_range() {
        let mut e = Encounter::default();
        e.add_group();
        e.groups[0].min_difficulty = Tier::Low;
        e.groups[0].max_difficulty = Tier::High;
        let xml = e.to_xml();
        assert!(xml.contains(r#"<group name="Group 1" minDifficulty="low" maxDifficulty="high">"#), "{xml}");
        assert!(xml.contains(r#"<group name="Group 2" minDifficulty="moderate" maxDifficulty="moderate">"#), "{xml}");
        assert!(!xml.contains("<encounter id=\"new-encounter\" level=\"1\" numPCs=\"4\" min"), "{xml}");
        assert_eq!(Encounter::parse(&xml).unwrap(), e);
    }

    #[test]
    fn difficulty_defaults_moderate_or_the_older_encounter_wide_range() {
        let plain = r#"<encounter id="x" level="2" numPCs="4"><group name="g"><baseMonsterRoster/></group><reference/></encounter>"#;
        let g = &Encounter::parse(plain).unwrap().groups[0];
        assert_eq!((g.min_difficulty, g.max_difficulty), (Tier::Moderate, Tier::Moderate));

        // Saved before the range moved to groups: it applies to every group
        // that doesn't set its own.
        let older = r#"<encounter id="x" level="2" numPCs="4" minDifficulty="low" maxDifficulty="high">
            <group name="a"><baseMonsterRoster/></group>
            <group name="b" maxDifficulty="moderate"><baseMonsterRoster/></group><reference/></encounter>"#;
        let e = Encounter::parse(older).unwrap();
        assert_eq!((e.groups[0].min_difficulty, e.groups[0].max_difficulty), (Tier::Low, Tier::High));
        assert_eq!((e.groups[1].min_difficulty, e.groups[1].max_difficulty), (Tier::Low, Tier::Moderate));

        assert!(Encounter::parse(&plain.replace(r#"<group name="g""#, r#"<group name="g" maxDifficulty="deadly""#)).is_err());
    }

    #[test]
    fn adding_monsters_creates_references_and_counts() {
        let mut e = Encounter::default();
        let at = RosterRef::FIRST;
        assert_eq!(e.add_monster(at, "res://srd/goblin.xml", "goblin").as_deref(), Some("goblin"));
        e.add_monster(at, "res://srd/goblin.xml", "goblin");
        assert_eq!(e.groups[0].base_roster, vec![RosterEntry { id: "goblin".into(), count: 2 }]);
        assert_eq!(e.references.len(), 1);
        // Same monster id from another file: its own reference, renamed.
        assert_eq!(e.add_monster(at, "res://mine/goblin.xml", "goblin").as_deref(), Some("goblin-2"));
        assert_eq!(e.references[1].file, "res://mine/goblin.xml");
        // A roster that isn't there.
        assert_eq!(e.add_monster(RosterRef { group: 5, roster: RosterSel::Base }, "f", "m"), None);
    }

    #[test]
    fn alt_rosters_copy_the_base_one_level_up() {
        let mut e = Encounter::default();
        e.level = 3;
        e.add_monster(RosterRef::FIRST, "f", "orc");
        let at = e.add_alt_roster(0).unwrap();
        assert_eq!(at.roster, RosterSel::Alt(0));
        assert_eq!(e.party(at), Some((4, 4)));
        assert_eq!(e.roster(at), e.roster(RosterRef::FIRST));
        let next = e.add_alt_roster(0).unwrap();
        assert_eq!(e.party(next), Some((5, 4)));
    }

    #[test]
    fn unused_references_and_problems() {
        let mut e = Encounter::default();
        assert!(e.problems().iter().any(|p| p.contains("no monsters yet")));
        e.add_monster(RosterRef::FIRST, "f", "orc");
        e.groups[0].base_roster.clear();
        assert_eq!(e.unused_references(), vec!["orc"]);
        assert!(e.problems().iter().any(|p| p.contains("base roster has no monsters")));
        e.add_monster(RosterRef::FIRST, "f", "orc");
        e.add_alt_roster(0);
        e.add_alt_roster(0);
        e.groups[0].alt_rosters[1].level = e.groups[0].alt_rosters[0].level;
        assert!(e.problems().iter().any(|p| p.contains("two alternate rosters")));
    }
}
