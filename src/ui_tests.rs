//! Headless UI tests: the real `view()` driven by `iced_test`'s simulator,
//! with every message fed back through `update()` and every returned `Task`
//! run to completion, so folder scans and (scripted) file dialogs happen for
//! real. Native dialogs and window-system behaviour are left to manual use.

use super::*;
use crate::config::MonsterDir;
use crate::config_modal::{self, DIR_PATH_INPUT};
use iced::keyboard::{Key, Modifiers, key::Named};
use iced_test::selector::{Candidate, Selector};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ----- harness ---------------------------------------------------------------

struct Harness {
    app: App,
    dialogs: Dialogs,
    root: tempfile::TempDir,
    /// Stat block renders so far (the fake compiler's calls).
    renders: Arc<AtomicUsize>,
    /// When set, the fake compiler fails with this message.
    render_error: Arc<Mutex<Option<String>>>,
    /// The last request the fake compiler got.
    last_request: Arc<Mutex<Option<statblock::Request>>>,
    /// Where the (simulated) mouse is, during drags.
    cursor: Option<iced::Point>,
}

impl Harness {
    /// An app over a temporary resource folder:
    ///
    /// ```text
    /// srd/aboleth.xml            Aboleth (CR 10, 11 in lair)
    /// srd/goblin.xml             Goblin (CR 1/4)
    /// monsters/bandits.xml       "Bandits": Bandit Sniper, Bandit Deadeye
    /// monsters/sub/goblin.xml    Goblin (CR 1)  — same name as the SRD one
    /// encounters/bridge.xml      the sample encounter
    /// ```
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let res = root.path().join("res");
        write(&res.join("srd/aboleth.xml"), &monster_file(None, &[("aboleth", "Aboleth", &["10", "11|in lair"])]));
        write(&res.join("srd/goblin.xml"), &monster_file(None, &[("goblin", "Goblin", &["1/4"])]));
        write(
            &res.join("monsters/bandits.xml"),
            &monster_file(
                Some("Bandits"),
                &[("bandit-sniper", "Bandit Sniper", &["1/2"]), ("bandit-deadeye", "Bandit Deadeye", &["2"])],
            ),
        );
        write(&res.join("monsters/sub/goblin.xml"), &monster_file(None, &[("goblin-boss", "Goblin", &["1"])]));
        write(
            &res.join("encounters/bridge.xml"),
            &fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/encounters.xml")).unwrap(),
        );
        write(&res.join("encounters/broken.xml"), "<encounter id=\"x\"");

        let config = Config {
            resource_dir: res.to_string_lossy().replace('\\', "/"),
            srd_monster_dir: MonsterDir { path: "res://srd".into(), include_subdirs: false },
            monster_dirs: vec![MonsterDir { path: "res://monsters".into(), include_subdirs: true }],
            ..Config::default()
        };
        let dialogs = Dialogs::scripted();
        let (mut app, boot) = App::with_config(config, root.path().join("config.toml"), dialogs.clone());
        app.window_width = INITIAL_SIZE.width; // the simulator's window: the app's starting size

        // Stat blocks come from a fake compiler that writes a tiny PNG.
        let renders = Arc::new(AtomicUsize::new(0));
        let render_error = Arc::new(Mutex::new(None::<String>));
        let last_request = Arc::new(Mutex::new(None));
        app.cache_root = root.path().join("cache");
        app.renderer = statblock::Renderer::Fake({
            let (renders, render_error, last_request) = (renders.clone(), render_error.clone(), last_request.clone());
            Arc::new(move |request: &statblock::Request, out: &Path| {
                renders.fetch_add(1, Ordering::SeqCst);
                *last_request.lock().unwrap() = Some(request.clone());
                if let Some(e) = render_error.lock().unwrap().clone() {
                    return Err(e);
                }
                fs::write(out.join("page0.png"), statblock::tests::TINY_PNG).map_err(|e| e.to_string())
            })
        });

        let mut h = Harness { app, dialogs, root, renders, render_error, last_request, cursor: None };
        h.run(boot);
        h
    }

    fn res(&self, rel: &str) -> PathBuf {
        self.root.path().join("res").join(rel)
    }

    /// Runs a task to completion, feeding each message it produces back
    /// through `update` (and running the tasks those return).
    fn run(&mut self, task: Task<Message>) {
        let Some(stream) = iced_test::runtime::task::into_stream(task) else {
            return;
        };
        // Runtime actions (window queries, focus) are dropped; a dropped
        // reply channel simply ends that part of the stream.
        let outputs: Vec<Message> = futures::executor::block_on_stream(stream)
            .filter_map(|action| match action {
                iced_test::runtime::Action::Output(m) => Some(m),
                _ => None,
            })
            .collect();
        for m in outputs {
            self.send(m);
        }
    }

    fn send(&mut self, message: Message) {
        let task = self.app.update(message);
        self.run(task);
    }

    /// Simulates on a fresh render of the current view, then processes the
    /// messages the interaction produced.
    fn interact(&mut self, f: impl FnOnce(&mut iced_test::Simulator<'_, Message>)) {
        let messages: Vec<Message> = {
            let mut ui = sim(self.app.view(), self.app.window_width);
            f(&mut ui);
            ui.into_messages().collect()
        };
        for m in messages {
            self.send(m);
        }
    }

    /// Clicks the widget showing exactly this text. When several do (the
    /// ribbon's "Save" and a modal's "Save"), the last one drawn wins: it is
    /// the one on top, as a user would see it.
    fn click(&mut self, text: &str) {
        self.interact(|ui| click_topmost(ui, text));
    }

    /// Clicks the nth (from 1) widget showing exactly this text.
    fn click_nth(&mut self, text: &str, nth: usize) {
        self.interact(|ui| {
            let mut seen = 0;
            ui.click(|c: Candidate<'_>| {
                let mut text = text;
                let target = text.select(c)?;
                seen += 1;
                (seen == nth).then_some(target)
            })
            .unwrap_or_else(|e| panic!("couldn't click {text:?} number {nth}: {e:?}"));
        });
    }

    fn double_click(&mut self, text: &str) {
        self.interact(|ui| {
            click_topmost(ui, text);
            click_topmost(ui, text);
        });
    }

    /// Types into a text input. Each key goes through a fresh render, as
    /// it would in the running app, since an input's value only changes
    /// once `update` has run.
    fn type_into(&mut self, id: &'static str, text: &str) {
        for ch in text.chars() {
            self.interact(|ui| {
                ui.click(iced::widget::Id::from(id)).unwrap_or_else(|e| panic!("no input {id}: {e:?}"));
                let _ = ui.tap_key(Key::Named(Named::End));
                let _ = ui.typewrite(&ch.to_string());
            });
        }
    }

    fn renders(&self) -> usize {
        self.renders.load(Ordering::SeqCst)
    }

    fn statblock(&self, file: &str, monster: &str) -> Option<&Statblock> {
        self.app.statblocks.get(&statblock::Key { file: self.res(file), monster: monster.into() })
    }

    fn shows(&mut self, text: &str) -> bool {
        sim(self.app.view(), self.app.window_width).find(text).is_ok()
    }

    fn assert_shows(&mut self, text: &str) {
        assert!(self.shows(text), "expected {text:?} on screen");
    }

    fn assert_hides(&mut self, text: &str) {
        assert!(!self.shows(text), "expected {text:?} not on screen");
    }
}

/// A simulator as wide as the app thinks its window is (its starting size
/// unless a test changes it).
fn sim(view: Element<'_, Message>, width: f32) -> iced_test::Simulator<'_, Message> {
    iced_test::Simulator::with_size(iced::Settings::default(), Size::new(width, INITIAL_SIZE.height), view)
}

fn click_topmost(ui: &mut iced_test::Simulator<'_, Message>, mut text: &str) {
    let mut count = 0;
    let _ = ui.find(|c: Candidate<'_>| {
        if text.select(c).is_some() {
            count += 1;
        }
        None::<()>
    });
    assert!(count > 0, "nothing on screen reads {text:?}");
    let mut seen = 0;
    ui.click(|c: Candidate<'_>| {
        let target = text.select(c)?;
        seen += 1;
        (seen == count).then_some(target)
    })
    .unwrap_or_else(|e| panic!("couldn't click {text:?}: {e:?}"));
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A monster family file. CRs are "value" or "value|note".
fn monster_file(family: Option<&str>, monsters: &[(&str, &str, &[&str])]) -> String {
    let mut s = String::from("<monsters>\n");
    if let Some(f) = family {
        s += &format!("  <name>{f}</name>\n");
    }
    for (id, name, crs) in monsters {
        s += &format!("  <monster id=\"{id}\">\n    <name>{name}</name>\n    <attributes><challengeRatings>\n");
        for cr in *crs {
            match cr.split_once('|') {
                Some((v, note)) => s += &format!("      <challengeRating value=\"{v}\" note=\"{note}\"/>\n"),
                None => s += &format!("      <challengeRating value=\"{cr}\"/>\n"),
            }
        }
        s += "    </challengeRatings></attributes>\n  </monster>\n";
    }
    s + "</monsters>\n"
}

// ----- monster browser -------------------------------------------------------

#[test]
fn browser_lists_monsters_as_name_and_base_cr() {
    let mut h = Harness::new();
    h.assert_shows("Aboleth (CR 10)"); // the base CR, not the lair one
    h.assert_shows("▾ SRD · res://srd  (2)");
    h.assert_shows("▾ res://monsters*  (3)");
}

#[test]
fn file_path_shows_only_for_names_in_several_files() {
    let mut h = Harness::new();
    h.assert_shows("goblin.xml"); // SRD Goblin
    h.assert_shows("sub/goblin.xml"); // the other Goblin
    h.assert_hides("aboleth.xml");
}

#[test]
fn multi_monster_file_is_a_collapsed_heading_until_clicked() {
    let mut h = Harness::new();
    h.assert_shows("▸ Bandits  (2)");
    h.assert_hides("Bandit Sniper (CR 1/2)");

    h.click("▸ Bandits  (2)");
    h.assert_shows("▾ Bandits  (2)");
    h.assert_shows("Bandit Sniper (CR 1/2)");
    h.assert_shows("Bandit Deadeye (CR 2)");

    h.click("▾ Bandits  (2)");
    h.assert_hides("Bandit Sniper (CR 1/2)");
}

#[test]
fn folder_heading_collapses_and_expands() {
    let mut h = Harness::new();
    h.click("▾ SRD · res://srd  (2)");
    h.assert_shows("▸ SRD · res://srd  (2)");
    h.assert_hides("Aboleth (CR 10)");
    h.assert_shows("Goblin (CR 1)"); // the other folder is untouched

    h.click("▸ SRD · res://srd  (2)");
    h.assert_shows("Aboleth (CR 10)");
}

#[test]
fn filter_narrows_the_list_and_opens_file_headings() {
    let mut h = Harness::new();
    h.type_into(MONSTER_FILTER_INPUT, "snip");
    assert_eq!(h.app.filter, "snip");
    h.assert_shows("Bandit Sniper (CR 1/2)");
    h.assert_hides("Bandit Deadeye (CR 2)");
    h.assert_hides("Aboleth (CR 10)");
    h.assert_shows("▾ SRD · res://srd  (0)");
    h.assert_shows("No matches");
}

#[test]
fn selecting_a_monster_records_it() {
    let mut h = Harness::new();
    h.click("Aboleth (CR 10)");
    assert_eq!(h.app.selected_monster, Some((h.res("srd/aboleth.xml"), "aboleth".into())));
}

#[test]
fn missing_folder_shows_an_error_under_its_heading() {
    let mut h = Harness::new();
    h.app.config.monster_dirs.push(MonsterDir { path: "res://nowhere".into(), include_subdirs: false });
    h.send(Message::Rescan);
    h.assert_shows("▾ res://nowhere  (0)");
    assert!(h.app.sources.last().unwrap().error.is_some());
}

// ----- left-hand side panel ----------------------------------------------------

#[test]
fn vertical_tab_toggles_the_panel() {
    let mut h = Harness::new();
    let tab = "M\nO\nN\nS\nT\nE\nR\nS";
    h.assert_shows("Monsters");
    h.click(tab);
    assert_eq!(h.app.lhs, None);
    h.assert_hides("Monsters");
    h.click(tab);
    h.assert_shows("Monsters");
}

#[test]
fn divider_drag_resizes_within_limits() {
    let mut h = Harness::new();
    h.send(Message::DividerPressed(Divider::Lhs));
    h.send(Message::DragTo(TAB_STRIP_WIDTH + 700.0));
    assert!((h.app.lhs_ratio - 0.5).abs() < 1e-4);
    h.send(Message::DragTo(5000.0));
    assert_eq!(h.app.lhs_ratio, 0.7);
    h.send(Message::DragEnd);
    h.send(Message::DragTo(TAB_STRIP_WIDTH + 100.0)); // not dragging: ignored
    assert_eq!(h.app.lhs_ratio, 0.7);
}

// ----- documents -------------------------------------------------------------

#[test]
fn new_then_save_asks_for_a_path_and_writes_the_file() {
    let mut h = Harness::new();
    h.click("New");
    h.assert_shows("new-encounter (unsaved) •");

    let out = h.root.path().join("out.xml");
    h.dialogs.answer(vec![out.clone()]);
    h.click("Save");
    h.assert_shows("out.xml");
    let saved = Encounter::parse(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(saved, Encounter::default());
}

#[test]
fn cancelled_save_dialog_leaves_the_document_unsaved() {
    let mut h = Harness::new();
    h.click("New");
    h.dialogs.answer(vec![]);
    h.click("Save");
    h.assert_shows("new-encounter (unsaved) •");
}

#[test]
fn open_shows_the_encounter_and_does_not_open_it_twice() {
    let mut h = Harness::new();
    let bridge = h.res("encounters/bridge.xml");
    h.dialogs.answer(vec![bridge.clone()]);
    h.click("Open");
    h.assert_shows("bridge.xml");
    h.assert_shows("bridge-ambush"); // the id field
    h.assert_shows("Bridge Ambush"); // the group's name field
    h.assert_shows("Base roster · level 3 · 5 PCs");
    h.assert_shows("Alternate roster");
    // Its monsters live in folders the test resources don't have.
    h.assert_shows("bandit-sniper isn't in the monster folders");

    h.dialogs.answer(vec![bridge]);
    h.click("Open");
    assert_eq!(h.app.docs.len(), 1);
}

#[test]
fn open_reports_files_it_cannot_read() {
    let mut h = Harness::new();
    h.dialogs.answer(vec![h.res("encounters/broken.xml")]);
    h.click("Open");
    assert!(h.app.docs.is_empty());
    assert!(h.app.status.as_ref().is_some_and(|s| s.is_error && s.text.contains("broken.xml")));
}

#[test]
fn save_all_writes_saved_files_and_asks_for_new_ones() {
    let mut h = Harness::new();
    h.dialogs.answer(vec![h.res("encounters/bridge.xml")]);
    h.click("Open");
    h.click("New");
    let out = h.root.path().join("second.xml");
    h.dialogs.answer(vec![out.clone()]);
    h.click("Save All");
    assert!(out.exists());
    assert!(h.app.docs.iter().all(|d| !d.dirty && d.path.is_some()));
}

#[test]
fn closing_a_tab_activates_a_neighbour() {
    let mut h = Harness::new();
    h.click("New");
    h.click("New");
    let (first, second) = (h.app.docs[0].id, h.app.docs[1].id);
    assert_eq!(h.app.active_doc, Some(second));
    h.click("×"); // the last tab's close button: the active tab
    assert_eq!(h.app.docs.len(), 1);
    assert_eq!(h.app.docs[0].id, first);
    assert_eq!(h.app.active_doc, Some(first));
}

// ----- shortcuts ---------------------------------------------------------------

#[test]
fn shortcut_keys_map_to_commands() {
    let ctrl = Modifiers::CTRL;
    let key = |c: &str| Key::Character(c.into());
    assert_eq!(shortcut_for(&key("n"), ctrl), Some(Shortcut::New));
    assert_eq!(shortcut_for(&key("o"), ctrl), Some(Shortcut::Open));
    assert_eq!(shortcut_for(&key("s"), ctrl), Some(Shortcut::Save));
    assert_eq!(shortcut_for(&key("S"), ctrl | Modifiers::SHIFT), Some(Shortcut::SaveAll));
    assert_eq!(shortcut_for(&key("s"), Modifiers::empty()), None);
    assert_eq!(shortcut_for(&key("x"), ctrl), None);
}

#[test]
fn shortcuts_do_nothing_while_the_config_modal_is_open() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.send(Message::Shortcut(Shortcut::New));
    assert!(h.app.docs.is_empty());
}

// ----- configuration modal ---------------------------------------------------

#[test]
fn adding_a_folder_marks_subdirectories_and_saves_on_save() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.click("New…");
    h.type_into(DIR_PATH_INPUT, "res://extra");
    h.click("OK"); // "Include subdirectories" starts checked
    h.assert_shows("res://extra*");

    h.click("Save");
    assert!(h.app.config_editor.is_none());
    assert_eq!(
        h.app.config.monster_dirs.last(),
        Some(&MonsterDir { path: "res://extra".into(), include_subdirs: true })
    );
    let (on_disk, error) = Config::load_from(&h.root.path().join("config.toml"));
    assert!(error.is_none());
    assert_eq!(on_disk, h.app.config);
    h.assert_shows("▾ res://extra*  (0)"); // rescanned
}

#[test]
fn unticking_include_subdirectories_drops_the_star() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.click("New…");
    h.type_into(DIR_PATH_INPUT, "res://flat");
    // The editor's checkbox is drawn after (on top of) the config panel's.
    h.send(Message::Config(config_modal::Msg::EditorSubdirs(false)));
    h.click("OK");
    h.assert_shows("res://flat");
    h.assert_hides("res://flat*");
}

#[test]
fn double_clicking_a_folder_opens_it_for_editing() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.double_click("res://monsters*");
    h.assert_shows("Edit monster folder");
    h.assert_shows("res://monsters"); // the path field's value
}

#[test]
fn delete_and_reorder_folders() {
    let mut h = Harness::new();
    h.app.config.monster_dirs.push(MonsterDir { path: "res://second".into(), include_subdirs: false });
    h.click("Configuration");
    h.click("res://second");
    h.click("Move up");
    h.click("Save");
    assert_eq!(h.app.config.monster_dirs[0].path, "res://second");

    h.click("Configuration");
    h.click("res://monsters*");
    h.click("Delete");
    h.click("Save");
    assert_eq!(h.app.config.monster_dirs.len(), 1);
    assert_eq!(h.app.config.monster_dirs[0].path, "res://second");
}

#[test]
fn browse_turns_folders_under_the_resource_folder_into_res_paths() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.dialogs.answer(vec![h.res("srd/more")]);
    h.send(Message::Config(config_modal::Msg::Browse(config_modal::BrowseTarget::SrdDir)));
    h.assert_shows("res://srd/more");
}

#[test]
fn cancel_discards_config_changes() {
    let mut h = Harness::new();
    let before = h.app.config.clone();
    h.click("Configuration");
    h.click("New…");
    h.type_into(DIR_PATH_INPUT, "res://temp");
    h.click("OK");
    h.click("Cancel");
    assert!(h.app.config_editor.is_none());
    assert_eq!(h.app.config, before);
    assert!(!h.root.path().join("config.toml").exists());
}

// ----- monster viewer ----------------------------------------------------------

#[test]
fn selecting_a_monster_opens_its_stat_block() {
    let mut h = Harness::new();
    h.assert_hides("Recompile");
    h.click("Aboleth (CR 10)");
    assert_eq!(h.renders(), 1);
    assert!(matches!(h.statblock("srd/aboleth.xml", "aboleth"), Some(Statblock::Ready(images)) if images.len() == 1));
    h.assert_shows("Recompile");
    h.assert_shows("Close");
}

#[test]
fn viewer_says_compiling_until_the_compiler_finishes() {
    let mut h = Harness::new();
    // Select without running the compile task.
    let _pending = h.app.update(Message::SelectMonster(h.res("srd/aboleth.xml"), "aboleth".into()));
    h.assert_shows("Compiling stat block…");
}

#[test]
fn the_compiler_gets_the_monster_and_the_configured_compiler() {
    let mut h = Harness::new();
    h.click("▸ Bandits  (2)");
    h.click("Bandit Deadeye (CR 2)");
    let request = h.last_request.lock().unwrap().clone().unwrap();
    assert_eq!(request.key.file, h.res("monsters/bandits.xml"));
    assert_eq!(request.key.monster, "bandit-deadeye");
    assert_eq!(request.compiler_dir, h.res("compiler"));
    assert_eq!(request.uv_command, "uv");
}

#[test]
fn stat_blocks_are_cached_until_the_monster_file_changes() {
    let mut h = Harness::new();
    h.click("Aboleth (CR 10)");
    h.click("Goblin (CR 1/4)");
    h.click("Aboleth (CR 10)"); // in memory
    assert_eq!(h.renders(), 2);

    // As after a restart: the images on disk are reused. (Goblin first, so
    // the Aboleth row is the only "Aboleth (CR 10)" on screen to click,
    // not the viewer's title.)
    h.click("Goblin (CR 1/4)");
    h.app.statblocks.clear();
    h.click("Aboleth (CR 10)");
    assert_eq!(h.renders(), 2);
    assert!(matches!(h.statblock("srd/aboleth.xml", "aboleth"), Some(Statblock::Ready(_))));

    // Editing the monster's file makes its images stale.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options().write(true).open(h.res("srd/aboleth.xml")).unwrap().set_modified(later).unwrap();
    h.click("Goblin (CR 1/4)");
    h.app.statblocks.clear();
    h.click("Aboleth (CR 10)");
    assert_eq!(h.renders(), 3);
}

#[test]
fn recompile_always_runs_the_compiler() {
    let mut h = Harness::new();
    h.click("Aboleth (CR 10)");
    h.click("Recompile");
    assert_eq!(h.renders(), 2);
}

#[test]
fn compiler_errors_show_in_the_viewer() {
    let mut h = Harness::new();
    *h.render_error.lock().unwrap() = Some("The compiler failed:\nerror: no monster named lich".into());
    h.click("Aboleth (CR 10)");
    h.assert_shows("Couldn't draw this stat block");
    h.assert_shows("The compiler failed:\nerror: no monster named lich");

    // A failed stat block is tried again when picked again.
    *h.render_error.lock().unwrap() = None;
    h.click("Goblin (CR 1/4)");
    h.click("Aboleth (CR 10)");
    assert!(matches!(h.statblock("srd/aboleth.xml", "aboleth"), Some(Statblock::Ready(_))));
}

#[test]
fn close_hides_the_viewer() {
    let mut h = Harness::new();
    h.click("Aboleth (CR 10)");
    h.click("Close");
    assert_eq!(h.app.selected_monster, None);
    h.assert_hides("Recompile");
}

#[test]
fn viewer_divider_drag_resizes_within_limits() {
    let mut h = Harness::new();
    h.send(Message::DividerPressed(Divider::Rhs));
    h.send(Message::DragTo(700.0));
    assert!((h.app.rhs_ratio - 0.5).abs() < 1e-4);
    h.send(Message::DragTo(1350.0));
    assert_eq!(h.app.rhs_ratio, 0.15);
    h.send(Message::DragEnd);
    assert_eq!(h.app.dragging, None);
}

#[test]
fn compiler_folder_is_set_in_configuration() {
    let mut h = Harness::new();
    h.click("Configuration");
    h.assert_shows("res://compiler");
    h.dialogs.answer(vec![h.res("tools/encompile")]);
    h.send(Message::Config(config_modal::Msg::Browse(config_modal::BrowseTarget::CompilerDir)));
    h.assert_shows("res://tools/encompile");
    h.click("Save");
    assert_eq!(h.app.config.compiler_dir, "res://tools/encompile");
}

#[test]
fn a_stat_block_in_several_column_pieces_shows_them_all_in_order() {
    let mut h = Harness::new();
    h.app.renderer = statblock::Renderer::Fake(Arc::new(|_: &statblock::Request, out: &Path| {
        for n in [1, 0, 2] {
            fs::write(out.join(format!("page{n}.png")), statblock::tests::TINY_PNG).map_err(|e| e.to_string())?;
        }
        Ok(())
    }));
    h.click("Aboleth (CR 10)");
    assert!(matches!(h.statblock("srd/aboleth.xml", "aboleth"), Some(Statblock::Ready(images)) if images.len() == 3));
    let pages = statblock::cached(&h.app.cache_root, &statblock::Key { file: h.res("srd/aboleth.xml"), monster: "aboleth".into() }).unwrap();
    let names: Vec<_> = pages.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["page0.png", "page1.png", "page2.png"]);
}

// ----- encounter editor ----------------------------------------------------------

impl Harness {
    /// A new encounter, the Aboleth in the viewer, and one Aboleth added.
    fn new_with_aboleth() -> Self {
        let mut h = Harness::new();
        h.click("New");
        h.click("Aboleth (CR 10)");
        h.click("Add to roster");
        h
    }

    fn doc(&self) -> &Document {
        self.app.active().expect("an open encounter")
    }

    fn edit(&mut self, edit: editor::Edit) {
        let id = self.doc().id;
        self.send(Message::Edit(id, edit));
    }
}

const BASE: encounter::RosterRef = encounter::RosterRef::FIRST;

#[test]
fn a_new_encounter_shows_its_budgets() {
    let mut h = Harness::new();
    h.click("New");
    h.assert_shows("Base roster · level 1 · 4 PCs");
    h.assert_shows("Total 0 XP");
    h.assert_shows("Rated Empty");
    // New encounters aim for Moderate: above the Low budget, up to Moderate's.
    h.assert_shows("Target (Moderate): 201 – 300 XP");
    h.assert_shows("Add 201 – 300 XP");
    h.assert_shows("Budgets for level 1, 4 PCs: Low 200 · Moderate 300 · High 400");
    h.assert_shows("Adding here");
}

#[test]
fn add_to_roster_puts_the_viewed_monster_in_the_encounter() {
    let mut h = Harness::new_with_aboleth();
    h.assert_shows("Aboleth"); // the roster row
    h.assert_shows("5,900");
    h.assert_shows("Total 5,900 XP");
    h.assert_shows("1 monsters");
    h.assert_shows("Rated Over the High budget");
    h.assert_shows("5,600 XP over"); // the Moderate target tops out at 300
    let e = &h.doc().encounter;
    assert_eq!(e.groups[0].base_roster, vec![encounter::RosterEntry { id: "aboleth".into(), count: 1 }]);
    assert_eq!(e.references[0].file, "res://srd/aboleth.xml");
    assert_eq!(e.references[0].monster, "aboleth");
    assert!(h.doc().dirty);
}

#[test]
fn adding_the_same_monster_again_counts_it() {
    let mut h = Harness::new_with_aboleth();
    h.click("Add to roster");
    h.assert_shows("Total 11,800 XP");
    h.assert_shows("2 monsters");
    assert_eq!(h.doc().encounter.references.len(), 1);
}

#[test]
fn add_to_roster_needs_an_open_encounter() {
    let mut h = Harness::new();
    h.click("Aboleth (CR 10)");
    h.click("Add to roster"); // disabled: nothing happens
    assert!(h.app.docs.is_empty());
}

#[test]
fn monsters_go_to_the_roster_last_clicked() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster");
    // A copy of the base roster, a level up, which now takes new monsters.
    h.assert_shows("Alternate roster");
    assert_eq!(h.doc().encounter.groups[0].alt_rosters[0].level, 2);
    assert_eq!(h.doc().target.roster, encounter::RosterSel::Alt(0));

    h.click("Goblin (CR 1/4)");
    h.click("Add to roster");
    let g = &h.doc().encounter.groups[0];
    assert_eq!(g.alt_rosters[0].monsters.len(), 2);
    assert_eq!(g.base_roster.len(), 1);

    h.click("Base roster · level 1 · 4 PCs"); // clicking a roster picks it
    assert_eq!(h.doc().target, BASE);
    h.click("Add to roster");
    assert_eq!(h.doc().encounter.groups[0].base_roster.len(), 2);
}

#[test]
fn party_edits_change_the_budgets() {
    let mut h = Harness::new_with_aboleth();
    h.edit(editor::Edit::Level(10));
    h.edit(editor::Edit::Pcs(5));
    h.assert_shows("Base roster · level 10 · 5 PCs");
    h.assert_shows("Budgets for level 10, 5 PCs: Low 8,000 · Moderate 11,500 · High 15,500");
    h.assert_shows("Target (Moderate): 8,001 – 11,500 XP");
    h.assert_shows("Add 2,101 – 5,600 XP");
    h.assert_shows("Rated Low");
    h.edit(editor::Edit::Level(99));
    assert_eq!(h.doc().encounter.level, 20); // clamped
}

#[test]
fn alternate_roster_party_and_removal() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster");
    h.edit(editor::Edit::AltLevel(0, 0, 15));
    h.edit(editor::Edit::AltPcs(0, 0, 6));
    assert_eq!(h.doc().encounter.party(encounter::RosterRef { group: 0, roster: encounter::RosterSel::Alt(0) }), Some((15, 6)));
    h.edit(editor::Edit::RemoveAlt(0, 0));
    assert!(h.doc().encounter.groups[0].alt_rosters.is_empty());
    assert_eq!(h.doc().target, BASE); // it was the target
}

#[test]
fn counts_and_removing_rows() {
    let mut h = Harness::new_with_aboleth();
    h.edit(editor::Edit::Count(BASE, 0, 3));
    h.assert_shows("17,700");
    h.edit(editor::Edit::Count(BASE, 0, 0));
    assert_eq!(h.doc().encounter.groups[0].base_roster[0].count, 1); // at least one

    h.edit(editor::Edit::RemoveRow(BASE, 0));
    h.assert_shows("unused"); // the reference no roster uses
    h.click("Remove");
    assert!(h.doc().encounter.references.is_empty());
}

#[test]
fn display_names_replace_the_stat_block_name_in_rosters() {
    let mut h = Harness::new_with_aboleth();
    h.edit(editor::Edit::DisplayName(0, "The Deep One".into()));
    h.assert_shows("The Deep One");
    h.edit(editor::Edit::ShortName(0, "Deep One".into()));
    assert_eq!(h.doc().encounter.references[0].short_name.as_deref(), Some("Deep One"));
    h.edit(editor::Edit::DisplayName(0, String::new()));
    assert_eq!(h.doc().encounter.references[0].display_name, None); // blank: the stat block's
}

#[test]
fn clicking_a_roster_monster_shows_its_stat_block() {
    let mut h = Harness::new_with_aboleth();
    h.click("Goblin (CR 1/4)");
    h.click("Aboleth"); // the roster row
    assert_eq!(h.app.selected_monster, Some((h.res("srd/aboleth.xml"), "aboleth".into())));
}

#[test]
fn the_edited_encounter_saves_to_xml() {
    let mut h = Harness::new_with_aboleth();
    h.edit(editor::Edit::Id("deep-trouble".into()));
    h.edit(editor::Edit::GroupName(0, "The Pool".into()));
    h.click("▸ Bandits  (2)");
    h.click("Bandit Deadeye (CR 2)");
    h.click("Add to roster");
    h.click("Add to roster");
    let out = h.root.path().join("deep.xml");
    h.dialogs.answer(vec![out.clone()]);
    h.click("Save");

    let saved = Encounter::parse(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(saved, h.doc().encounter);
    assert_eq!(saved.id, "deep-trouble");
    assert_eq!(saved.groups[0].name, "The Pool");
    assert_eq!(saved.groups[0].base_roster[1], encounter::RosterEntry { id: "bandit-deadeye".into(), count: 2 });
    assert_eq!(saved.references[1].file, "res://monsters/bandits.xml");
    assert!(!h.doc().dirty);
}

#[test]
fn problems_list_what_the_schema_would_reject() {
    let mut h = Harness::new();
    h.click("New");
    h.assert_shows("⚠ The encounter has no monsters yet.");
    h.assert_shows("⚠ Group 1: the base roster has no monsters.");
    h.click("Aboleth (CR 10)");
    h.click("Add to roster");
    h.assert_hides("⚠ The encounter has no monsters yet.");
    h.edit(editor::Edit::Id("bad id!".into()));
    h.assert_shows("⚠ The encounter id can only use letters, digits, - and _.");
}

#[test]
fn groups_can_be_added_and_removed() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Group");
    assert_eq!(h.doc().encounter.groups.len(), 2);
    h.assert_shows("Group 2");
    assert_eq!(h.doc().target.group, 1); // the new group takes new monsters
    h.edit(editor::Edit::RemoveGroup(1));
    assert_eq!(h.doc().encounter.groups.len(), 1);
    assert_eq!(h.doc().target, BASE);
    h.edit(editor::Edit::RemoveGroup(0)); // the last group stays
    assert_eq!(h.doc().encounter.groups.len(), 1);
}

#[test]
fn a_small_window_keeps_the_editor_usable_with_both_panels_open() {
    let mut h = Harness::new();
    h.send(Message::WindowResized(Size::new(1024.0, 768.0)));
    h.click("New");
    h.click("Aboleth (CR 10)");
    h.click("Add to roster");
    h.edit(editor::Edit::RemoveRow(BASE, 0));
    h.edit(editor::Edit::Count(BASE, 0, 1)); // nothing there: no-op
    h.click("Add to roster");
    h.click("Goblin (CR 1/4)");
    h.click("Aboleth"); // the roster row's name is wide enough to click
    assert_eq!(h.app.selected_monster.as_ref().map(|(_, id)| id.as_str()), Some("aboleth"));
    h.edit(editor::Edit::RemoveRow(BASE, 0));
    h.click("Remove"); // the unused reference's button too
    assert!(h.doc().encounter.references.is_empty());
}

#[test]
fn the_party_steppers_fit_beside_the_viewer() {
    // Found by eye: the PCs "+" was clipped off the header with the viewer open.
    for width in [INITIAL_SIZE.width, 1024.0] {
        let mut h = Harness::new();
        h.send(Message::WindowResized(Size::new(width, 768.0)));
        h.click("New");
        h.click("Aboleth (CR 10)");
        // The header's "+"s come first: Level, then PCs.
        let messages: Vec<Message> = {
            let mut ui = sim(h.app.view(), width);
            let mut seen = 0;
            for _ in 0..2 {
                let mut n = 0;
                let target = seen;
                ui.click(|c: Candidate<'_>| {
                    let t = "+".select(c)?;
                    n += 1;
                    (n == target + 1).then_some(t)
                })
                .unwrap_or_else(|e| panic!("at {width}px, + number {}: {e:?}", target + 1));
                seen += 1;
            }
            ui.into_messages().collect()
        };
        for m in messages {
            h.send(m);
        }
        assert_eq!((h.doc().encounter.level, h.doc().encounter.num_pcs), (2, 5), "at {width}px");
    }
}

// ----- intended difficulty ---------------------------------------------------

#[test]
fn the_target_follows_the_intended_difficulty_range() {
    let mut h = Harness::new();
    h.click("New");
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::High));
    h.assert_shows("Target (Moderate to High): 201 – 400 XP");
    h.edit(editor::Edit::MinDifficulty(xp::Tier::Low));
    h.assert_shows("Target (Low to High): 1 – 400 XP");
    assert!(h.doc().dirty);
}

#[test]
fn difficulty_pickers_keep_the_range_the_right_way_up() {
    let mut h = Harness::new();
    h.click("New");
    h.edit(editor::Edit::MinDifficulty(xp::Tier::High)); // above the top: raises it
    assert_eq!((h.doc().encounter.min_difficulty, h.doc().encounter.max_difficulty), (xp::Tier::High, xp::Tier::High));
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::Low)); // below the bottom: lowers it
    assert_eq!((h.doc().encounter.min_difficulty, h.doc().encounter.max_difficulty), (xp::Tier::Low, xp::Tier::Low));
    h.assert_shows("Target (Low): 1 – 200 XP");
}

#[test]
fn the_spreadsheet_case_level_3_five_pcs_moderate_to_high() {
    let mut h = Harness::new();
    h.click("New");
    h.edit(editor::Edit::Level(3));
    h.edit(editor::Edit::Pcs(5));
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::High));
    h.click("▸ Bandits  (2)");
    h.click("Bandit Deadeye (CR 2)");
    h.click("Add to roster");
    h.click("Add to roster");
    h.click("Bandit Sniper (CR 1/2)");
    h.click("Add to roster");
    // 2 x 450 + 100 = 1,000 XP, against 751 – 2,000.
    h.assert_shows("Total 1,000 XP");
    h.assert_shows("Target (Moderate to High): 751 – 2,000 XP");
    h.assert_shows("In range · 1,000 XP to spare");
    h.assert_shows("Rated Moderate");
    // Each alternate roster is measured against its own party.
    h.click("+ Alternate roster"); // level 4, 5 PCs: 1,250 / 1,875 / 2,500
    h.assert_shows("Target (Moderate to High): 1,251 – 2,500 XP");
    h.assert_shows("Add 251 – 1,500 XP");
}

#[test]
fn the_difficulty_range_is_saved_with_the_encounter() {
    let mut h = Harness::new_with_aboleth();
    h.edit(editor::Edit::MinDifficulty(xp::Tier::Low));
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::High));
    let out = h.root.path().join("range.xml");
    h.dialogs.answer(vec![out.clone()]);
    h.click("Save");
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.contains(r#"minDifficulty="low" maxDifficulty="high""#), "{text}");

    // And read back when the file is opened again.
    h.click("×");
    h.dialogs.answer(vec![out]);
    h.click("Open");
    assert_eq!((h.doc().encounter.min_difficulty, h.doc().encounter.max_difficulty), (xp::Tier::Low, xp::Tier::High));
}

#[test]
fn choosing_the_difficulty_range_with_its_buttons() {
    let mut h = Harness::new();
    h.click("New");
    // Each end has Low | Moderate | High; the second set is the top of the range.
    h.click_nth("High", 2);
    assert_eq!(h.doc().encounter.max_difficulty, xp::Tier::High);
    h.click_nth("Low", 1);
    assert_eq!(h.doc().encounter.min_difficulty, xp::Tier::Low);
    h.assert_shows("Target (Low to High): 1 – 400 XP");
}

#[test]
fn viewer_buttons_work_at_any_window_width() {
    // Found by hand: in a narrow window the viewer's buttons drew but ignored
    // clicks. Check every button in the viewer's header across widths.
    for width in (800..=1500).step_by(50) {
        let width = width as f32;
        let mut h = Harness::new();
        h.send(Message::WindowResized(Size::new(width, 900.0)));
        h.click("New");
        h.click("Aboleth (CR 10)");
        h.click("Add to roster");
        assert_eq!(h.doc().encounter.references.len(), 1, "Add to roster at {width}px");
        h.click("Recompile");
        assert_eq!(h.renders(), 2, "Recompile at {width}px");
        h.click("Close");
        assert_eq!(h.app.selected_monster, None, "Close at {width}px");
    }
}

// ----- drag and drop -------------------------------------------------------------

impl Harness {
    /// The centre of the topmost widget showing this text.
    fn centre_of(&mut self, text: &str) -> iced::Point {
        let mut ui = sim(self.app.view(), self.app.window_width);
        let mut count = 0;
        let _ = ui.find(|c: Candidate<'_>| {
            let mut t = text;
            if t.select(c).is_some() {
                count += 1;
            }
            None::<()>
        });
        drop(ui);
        self.centre_of_nth(text, count)
    }

    /// The centre of the nth (from 1) widget showing this text.
    fn centre_of_nth(&mut self, text: &str, count: usize) -> iced::Point {
        let mut ui = sim(self.app.view(), self.app.window_width);
        let mut seen = 0;
        let target = ui
            .find(|c: Candidate<'_>| {
                let mut t = text;
                let found = t.select(c)?;
                seen += 1;
                (seen == count).then_some(found)
            })
            .unwrap_or_else(|e| panic!("nothing reads {text:?}: {e:?}"));
        target.visible_bounds().unwrap_or_else(|| panic!("{text:?} isn't visible")).center()
    }

    /// Presses the mouse on a monster in the browser (and holds it). The
    /// browser comes first on screen, so its row is the first match.
    fn press(&mut self, text: &str) {
        let at = self.centre_of_nth(text, 1);
        self.cursor = Some(at);
        self.interact(|ui| {
            ui.point_at(at);
            let _ = ui.simulate([iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        });
        assert!(self.app.drag.is_some(), "pressing {text:?} didn't start a drag");
    }

    /// Moves the held mouse to a point: what the drag subscription reports,
    /// plus the cursor move the widgets see (for drop targets' enter/exit).
    fn move_to(&mut self, at: iced::Point) {
        self.send(Message::DragMoved(at));
        let from = self.cursor.replace(at);
        self.interact(|ui| {
            // Each step renders afresh, so first put the cursor back where it
            // was (widgets learn what it was over), then move it: leaving a
            // drop target is then seen, as in the running app.
            if let Some(from) = from {
                ui.point_at(from);
                let _ = ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved { position: from })]);
            }
            ui.point_at(at); // the simulator's cursor is what widgets see
            let _ = ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved { position: at })]);
        });
    }

    /// Drags the held monster over the widget showing this text.
    fn drag_over(&mut self, text: &str) {
        let at = self.centre_of(text);
        // In steps, as a real drag arrives: the first move only sets the origin.
        self.move_to(at + iced::Vector::new(-30.0, 0.0));
        self.move_to(at);
    }

    /// Lets go of the mouse (the drag subscription's release).
    fn release(&mut self) {
        self.send(Message::DragReleased);
    }
}

#[test]
fn dragging_a_monster_onto_a_roster_adds_it() {
    let mut h = Harness::new();
    h.click("New");
    h.press("Aboleth (CR 10)");
    h.drag_over("Base roster · level 1 · 4 PCs");
    assert_eq!(h.app.drop_hover, Some(BASE));
    h.assert_shows("Release to add"); // the label following the cursor
    h.release();

    let e = &h.doc().encounter;
    assert_eq!(e.groups[0].base_roster, vec![encounter::RosterEntry { id: "aboleth".into(), count: 1 }]);
    assert_eq!(e.references[0].file, "res://srd/aboleth.xml");
    assert!(h.doc().dirty);
    assert!(h.app.drag.is_none());
    assert_eq!(h.app.selected_monster, None); // a drag isn't a click
    assert_eq!(h.renders(), 0);
    h.assert_hides("Release to add");
}

#[test]
fn a_drop_goes_to_the_roster_it_lands_on_not_the_add_target() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster"); // now the "Add to roster" target
    h.press("Goblin (CR 1/4)");
    h.drag_over("Base roster · level 1 · 4 PCs");
    h.release();
    let g = &h.doc().encounter.groups[0];
    assert_eq!(g.base_roster.len(), 2);
    assert_eq!(g.alt_rosters[0].monsters.len(), 1);
    assert_eq!(h.doc().target.roster, encounter::RosterSel::Alt(0)); // unchanged
}

#[test]
fn dropping_a_monster_twice_counts_it() {
    let mut h = Harness::new();
    h.click("New");
    for _ in 0..2 {
        h.press("Aboleth (CR 10)");
        h.drag_over("Base roster · level 1 · 4 PCs");
        h.release();
    }
    h.assert_shows("Total 11,800 XP");
}

#[test]
fn dropping_off_a_roster_does_nothing() {
    let mut h = Harness::new();
    h.click("New");
    h.press("Aboleth (CR 10)");
    h.drag_over("Base roster · level 1 · 4 PCs");
    h.drag_over("Monster references"); // and on, off the roster
    assert_eq!(h.app.drop_hover, None);
    h.assert_shows("Drop on a roster");
    h.release();
    assert!(h.doc().encounter.references.is_empty());
    assert!(h.app.drag.is_none());
}

#[test]
fn a_press_that_barely_moves_is_still_a_click() {
    let mut h = Harness::new();
    h.click("New");
    let at = h.centre_of("Aboleth (CR 10)");
    h.press("Aboleth (CR 10)");
    h.move_to(at);
    h.move_to(at + iced::Vector::new(3.0, 2.0)); // within the threshold
    h.release();
    assert_eq!(h.app.selected_monster, Some((h.res("srd/aboleth.xml"), "aboleth".into())));
    assert!(h.doc().encounter.references.is_empty());
}

#[test]
fn dragging_with_no_encounter_open_does_nothing() {
    let mut h = Harness::new();
    h.press("Aboleth (CR 10)");
    h.drag_over("Goblin (CR 1/4)");
    h.release();
    assert!(h.app.docs.is_empty());
    assert!(h.app.drag.is_none());
}

#[test]
fn add_to_roster_still_works_beside_dragging() {
    let mut h = Harness::new();
    h.click("New");
    h.press("Goblin (CR 1/4)");
    h.drag_over("Base roster · level 1 · 4 PCs");
    h.release();
    h.click("Aboleth (CR 10)");
    h.click("Add to roster");
    assert_eq!(h.doc().encounter.groups[0].base_roster.len(), 2);
}

// ----- choosing a roster by clicking it ----------------------------------------

#[test]
fn clicking_a_roster_picks_it_for_add_to_roster() {
    let mut h = Harness::new_with_aboleth();
    h.assert_hides("Add monsters here"); // no button: the roster itself is the target
    h.click("+ Alternate roster");
    assert_eq!(h.doc().target.roster, encounter::RosterSel::Alt(0));
    h.click("Base roster · level 1 · 4 PCs");
    assert_eq!(h.doc().target, BASE);
    h.click("Alternate roster");
    assert_eq!(h.doc().target.roster, encounter::RosterSel::Alt(0));
    // Picking a roster isn't an edit of the file.
    let before = h.doc().encounter.clone();
    h.click("Base roster · level 1 · 4 PCs");
    assert_eq!(h.doc().encounter, before);
}

#[test]
fn clicking_empty_space_in_a_roster_picks_it() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster");
    // A point inside the base roster's box, away from any button: just right
    // of its title text.
    let title = h.centre_of("Base roster · level 1 · 4 PCs");
    let at = title + iced::Vector::new(200.0, 0.0);
    h.interact(|ui| {
        ui.point_at(at);
        let _ = ui.simulate(iced_test::simulator::click());
    });
    assert_eq!(h.doc().target, BASE);
}

#[test]
fn working_in_a_roster_picks_it() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster"); // the target
    h.edit(editor::Edit::Count(BASE, 0, 2));
    assert_eq!(h.doc().target, BASE);
    h.edit(editor::Edit::AltPcs(0, 0, 5));
    assert_eq!(h.doc().target.roster, encounter::RosterSel::Alt(0));
    h.edit(editor::Edit::RemoveRow(BASE, 0));
    assert_eq!(h.doc().target, BASE);
}

#[test]
fn clicking_a_monster_in_a_roster_picks_the_roster_and_shows_it() {
    let mut h = Harness::new_with_aboleth();
    h.click("+ Alternate roster"); // the target; both rosters list the Aboleth
    h.click("Goblin (CR 1/4)");
    h.click_nth("Aboleth", 1); // the base roster's row
    assert_eq!(h.doc().target, BASE);
    assert_eq!(h.app.selected_monster, Some((h.res("srd/aboleth.xml"), "aboleth".into())));
}

#[test]
fn raising_the_intended_difficulty_never_shrinks_what_can_be_added() {
    // Reported: level 3, 5 PCs, 300 XP of monsters. Low to High read "1,700 XP
    // to spare"; raising the bottom to Moderate read "451 XP short".
    let mut h = Harness::new();
    h.click("New");
    h.edit(editor::Edit::Level(3));
    h.edit(editor::Edit::Pcs(5));
    h.click("▸ Bandits  (2)");
    h.click("Bandit Sniper (CR 1/2)");
    h.click("Add to roster");
    h.click("Add to roster");
    h.click("Add to roster"); // 300 XP
    h.edit(editor::Edit::MinDifficulty(xp::Tier::Low));
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::High));
    h.assert_shows("In range · 1,700 XP to spare");
    h.edit(editor::Edit::MinDifficulty(xp::Tier::Moderate));
    h.assert_shows("Add 451 – 1,700 XP"); // still up to 1,700 more
    h.edit(editor::Edit::MaxDifficulty(xp::Tier::Moderate));
    h.assert_shows("Add 451 – 825 XP");
}
