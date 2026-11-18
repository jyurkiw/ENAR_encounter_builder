//! Stat block images for the monster viewer, drawn by the encompile compiler.
//!
//! A monster is compiled on its own (`encompile -m ID -c 2`) to a PDF and
//! cropped to its stat block by `scripts/statblock.py`, which this program
//! embeds and writes to its cache folder. The PNGs are cached per monster and
//! reused until the monster's file changes.
//!
//! The compiler runs through `uv run --project <compiler>` with its own Python
//! environment in our cache folder, so the compiler's `.venv` is never touched.

use std::path::{Path, PathBuf};

const SCRIPT: &str = include_str!("../scripts/statblock.py");
const DPI: &str = "150";

/// Which monster: its file and its `id` there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub file: PathBuf,
    pub monster: String,
}

/// Everything a render needs, gathered on the UI side.
#[derive(Debug, Clone)]
pub struct Request {
    pub key: Key,
    pub compiler_dir: PathBuf,
    pub uv_command: String,
    pub cache_root: PathBuf,
}

/// How stat blocks are drawn.
#[derive(Clone, Default)]
pub enum Renderer {
    /// The real compiler, through uv.
    #[default]
    Compiler,
    /// A stand-in for tests: called with the request and the folder to
    /// write PNGs to.
    #[cfg(test)]
    Fake(std::sync::Arc<dyn Fn(&Request, &Path) -> Result<(), String> + Send + Sync>),
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Renderer::Compiler => f.write_str("Compiler"),
            #[cfg(test)]
            Renderer::Fake(_) => f.write_str("Fake"),
        }
    }
}

/// The program's cache folder.
pub fn default_cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("encounter-designer")
}

/// Where a monster's images live: readable, and unique per file, id and
/// version of the driver script (so a change to how stat blocks are drawn
/// doesn't reuse images drawn the old way).
pub fn out_dir(cache_root: &Path, key: &Key) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.file.hash(&mut h);
    SCRIPT.hash(&mut h);
    let stem = key.file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect::<String>();
    cache_root
        .join("statblocks")
        .join(format!("{}-{}-{:08x}", clean(&stem), clean(&key.monster), h.finish() as u32))
}

/// The cached images, if there are any newer than the monster's file.
pub fn cached(cache_root: &Path, key: &Key) -> Option<Vec<PathBuf>> {
    let source = std::fs::metadata(&key.file).and_then(|m| m.modified()).ok()?;
    let pages = pages(&out_dir(cache_root, key));
    let fresh = !pages.is_empty()
        && pages.iter().all(|p| {
            std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t >= source)
        });
    fresh.then_some(pages)
}

/// page0.png, page1.png, ... in order.
fn pages(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name.strip_prefix("page")?.strip_suffix(".png")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    pages.sort();
    pages.into_iter().map(|(_, p)| p).collect()
}

/// Reads images into memory. Loading from bytes (not a path) makes iced
/// show a recompiled image instead of its cached copy of the old one.
pub fn load(paths: &[PathBuf]) -> Result<Vec<Vec<u8>>, String> {
    paths
        .iter()
        .map(|p| std::fs::read(p).map_err(|e| format!("Couldn't read {}: {e}", p.display())))
        .collect()
}

/// Draws a monster's stat block and returns its images' bytes.
pub async fn render(renderer: Renderer, request: Request) -> Result<Vec<Vec<u8>>, String> {
    let out = out_dir(&request.cache_root, &request.key);
    std::fs::create_dir_all(&out).map_err(|e| format!("Couldn't create {}: {e}", out.display()))?;
    match renderer {
        Renderer::Compiler => run_compiler(&request, &out).await?,
        #[cfg(test)]
        Renderer::Fake(f) => f(&request, &out)?,
    }
    let pages = pages(&out);
    if pages.is_empty() {
        return Err("The compiler made no image".into());
    }
    load(&pages)
}

async fn run_compiler(request: &Request, out: &Path) -> Result<(), String> {
    if !request.compiler_dir.join("encompile").is_dir() {
        return Err(format!(
            "No encompile compiler in {}. Set its folder in Configuration.",
            request.compiler_dir.display()
        ));
    }
    let script = request.cache_root.join("statblock.py");
    if std::fs::read_to_string(&script).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&script, SCRIPT).map_err(|e| format!("Couldn't write {}: {e}", script.display()))?;
    }

    let mut cmd = tokio::process::Command::new(&request.uv_command);
    cmd.arg("run")
        .arg("--project")
        .arg(&request.compiler_dir)
        .arg("python")
        .arg(&script)
        .arg(&request.compiler_dir)
        .arg(&request.key.file)
        .arg(&request.key.monster)
        .arg(out)
        .arg(DPI)
        .env("UV_PROJECT_ENVIRONMENT", request.cache_root.join("python-env"))
        .env("PYTHONIOENCODING", "utf-8")
        .current_dir(&request.compiler_dir);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up

    let output = cmd.output().await.map_err(|e| {
        format!(
            "Couldn't run uv ({}): {e}. Install uv, or set uv_command in the config file.",
            request.uv_command
        )
    })?;
    if output.status.success() {
        return Ok(());
    }
    // The compiler's own errors start with "error:"; show those, or the tail.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let errors: Vec<&str> = stderr.lines().filter(|l| l.trim_start().starts_with("error:")).collect();
    let detail = if errors.is_empty() {
        stderr.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    } else {
        errors.join("\n")
    };
    Err(format!("The compiler failed:\n{}", detail.trim()))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A 1x1 PNG, for fake renders.
    pub const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
        0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
        0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn cache_is_fresh_only_when_newer_than_the_monster_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lich.xml");
        std::fs::write(&file, "<monsters/>").unwrap();
        let key = Key { file: file.clone(), monster: "lich".into() };
        assert!(cached(dir.path(), &key).is_none());

        let out = out_dir(dir.path(), &key);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("page1.png"), TINY_PNG).unwrap();
        std::fs::write(out.join("page0.png"), TINY_PNG).unwrap();
        assert_eq!(cached(dir.path(), &key), Some(vec![out.join("page0.png"), out.join("page1.png")]));

        // Touch the monster file after the images: stale.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
        assert!(cached(dir.path(), &key).is_none());
    }

    #[test]
    fn out_dirs_differ_per_file_and_monster() {
        let root = Path::new("cache");
        let a = Key { file: "e:/a/goblin.xml".into(), monster: "goblin".into() };
        let b = Key { file: "e:/b/goblin.xml".into(), monster: "goblin".into() };
        let c = Key { file: "e:/a/goblin.xml".into(), monster: "boss".into() };
        assert_ne!(out_dir(root, &a), out_dir(root, &b));
        assert_ne!(out_dir(root, &a), out_dir(root, &c));
        assert!(out_dir(root, &a).to_string_lossy().contains("goblin-goblin-"));
    }

    /// Runs the real compiler on the SRD lich. Needs uv, LaTeX with the dnd
    /// template and the enigma-narrative repository: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_compiler_draws_the_lich() {
        let config = crate::config::Config::default();
        let cache = tempfile::tempdir().unwrap();
        let request = Request {
            key: Key { file: config.resolve("res://shared/internal/monsters/lich.xml"), monster: "lich".into() },
            compiler_dir: config.resolve(&config.compiler_dir),
            uv_command: config.uv_command.clone(),
            cache_root: cache.path().to_path_buf(),
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let images = rt.block_on(render(Renderer::Compiler, request)).unwrap();
        assert_eq!(images.len(), 2); // the lich's stat block runs on into a second column
        assert!(images.iter().all(|i| i.starts_with(&TINY_PNG[..8]))); // PNGs
    }
}
