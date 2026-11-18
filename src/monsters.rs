//! Finds monster family files (`<monsters>` roots) in the configured folders.

use crate::config::{Config, MonsterDir};
use std::path::{Path, PathBuf};

/// One configured folder and the monster files found in it.
#[derive(Debug, Clone)]
pub struct MonsterSource {
    pub dir: MonsterDir,
    pub is_srd: bool,
    pub files: Vec<MonsterFile>,
    /// Why the folder couldn't be read, if it couldn't.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonsterFile {
    pub path: PathBuf,
    /// The path relative to its source folder, for display.
    pub relative: String,
    /// The family's `<name>`.
    pub family: Option<String>,
    pub monsters: Vec<MonsterEntry>,
    /// Why the file couldn't be parsed, if it couldn't.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonsterEntry {
    pub id: String,
    pub name: String,
    /// The base challenge rating (the one without a note such as "in lair").
    pub cr: Option<String>,
}

impl MonsterEntry {
    /// "Name (CR X)", or just the name when the file gives no CR.
    pub fn label(&self) -> String {
        match &self.cr {
            Some(cr) => format!("{} (CR {cr})", self.name),
            None => self.name.clone(),
        }
    }
}

/// Scans the SRD folder (if set) and every monster folder.
pub fn scan(config: &Config) -> Vec<MonsterSource> {
    let srd = (!config.srd_monster_dir.path.trim().is_empty())
        .then(|| (config.srd_monster_dir.clone(), true));
    let extra = config.monster_dirs.iter().cloned().map(|d| (d, false));

    srd.into_iter()
        .chain(extra)
        .map(|(dir, is_srd)| {
            let root = config.resolve(&dir.path);
            let mut paths = Vec::new();
            let error = collect_xml(&root, dir.include_subdirs, &mut paths).err();
            paths.sort();
            let files = paths
                .into_iter()
                .filter_map(|path| read_monster_file(&root, path))
                .collect();
            MonsterSource { dir, is_srd, files, error }
        })
        .collect()
}

fn collect_xml(dir: &Path, recurse: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recurse {
                // A subfolder that can't be read shouldn't hide the rest.
                let _ = collect_xml(&path, true, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Reads a file's family name and monsters. Files whose root isn't
/// `<monsters>` aren't monster files and give `None`.
fn read_monster_file(root: &Path, path: PathBuf) -> Option<MonsterFile> {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut file = MonsterFile {
        path: path.clone(),
        relative,
        family: None,
        monsters: Vec::new(),
        error: None,
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            file.error = Some(e.to_string());
            return Some(file);
        }
    };
    let doc = match roxmltree::Document::parse(&text) {
        Ok(doc) => doc,
        Err(e) => {
            // Only report broken files that look like monster files.
            if !text.contains("<monsters") {
                return None;
            }
            file.error = Some(e.to_string());
            return Some(file);
        }
    };

    let root_el = doc.root_element();
    if root_el.tag_name().name() != "monsters" {
        return None;
    }
    let child_text = |node: roxmltree::Node, tag: &str| {
        node.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
    };
    file.family = child_text(root_el, "name");
    file.monsters = root_el
        .children()
        .filter(|c| c.has_tag_name("monster"))
        .map(|m| {
            let id = m.attribute("id").unwrap_or_default().to_string();
            let name = child_text(m, "name").unwrap_or_else(|| id.clone());
            let crs: Vec<_> = m.descendants().filter(|c| c.has_tag_name("challengeRating")).collect();
            let cr = crs
                .iter()
                .find(|c| c.attribute("note").is_none_or(|n| n.trim().is_empty()))
                .or(crs.first())
                .and_then(|c| c.attribute("value"))
                .map(str::to_string);
            MonsterEntry { id, name, cr }
        })
        .collect();
    Some(file)
}
