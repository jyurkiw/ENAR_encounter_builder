mod config;
mod config_modal;
mod description;
mod description_modal;
mod description_window;
mod dialogs;
mod editor;
mod encounter;
mod export_modal;
mod foundry;
mod monsters;
mod statblock;
mod xp;
mod ui;
#[cfg(test)]
mod ui_tests;

use config::Config;
use config_modal::ConfigEditor;
use dialogs::Dialogs;
use encounter::Encounter;
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input,
};
use iced::{
    Alignment, Element, Event, Length, Size, Subscription, Task, event, keyboard, mouse, window,
};
use monsters::MonsterSource;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const INITIAL_SIZE: Size = Size::new(1400.0, 900.0);
const TAB_STRIP_WIDTH: f32 = 30.0;
const DIVIDER_WIDTH: f32 = 9.0;
const DEFAULT_RHS_RATIO: f32 = 0.38;
/// The narrowest the encounter editor gets while the viewer is open.
const MIN_MAIN_WIDTH: f32 = 480.0;
/// How far (in pixels) a pressed monster moves before it's being dragged.
const DRAG_THRESHOLD: f32 = 5.0;
const MONSTER_FILTER_INPUT: &str = "monster-filter";

fn main() -> iced::Result {
    // A daemon, for more than one window: the main window, and the encounter
    // description's (see App::view_window). Closing the main window quits.
    iced::daemon(App::new, App::update, App::view_window)
        .title(App::window_title)
        .subscription(App::subscription)
        .run()
}

fn main_window_settings() -> window::Settings {
    window::Settings { size: INITIAL_SIZE, icon: window_icon(), ..window::Settings::default() }
}

fn description_window_settings() -> window::Settings {
    window::Settings { size: Size::new(960.0, 1000.0), icon: window_icon(), ..window::Settings::default() }
}

/// The program icon: a pixel-art crocodile (tools/make_icon.py draws it).
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn window_icon() -> Option<window::Icon> {
    window::icon::from_file_data(ICON_PNG, None).ok()
}

/// The left-hand side panels, one per vertical tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LhsTab {
    Monsters,
}

impl LhsTab {
    const ALL: [LhsTab; 1] = [LhsTab::Monsters];

    fn label(self) -> &'static str {
        match self {
            LhsTab::Monsters => "Monsters",
        }
    }
}

/// An open encounter file.
struct Document {
    id: u64,
    path: Option<PathBuf>,
    encounter: Encounter,
    dirty: bool,
    /// The roster "Add to roster" puts monsters in.
    target: encounter::RosterRef,
    /// Roster rows (roster, row) whose local name fields are open.
    local_names_open: HashSet<(encounter::RosterRef, usize)>,
}

impl Document {
    fn title(&self) -> String {
        let name = match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => format!("{} (unsaved)", self.encounter.id),
        };
        if self.dirty { format!("{name} •") } else { name }
    }
}

/// The panel dividers that can be dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Divider {
    Lhs,
    Rhs,
}

/// A monster's stat block in the viewer.
enum Statblock {
    Compiling,
    Ready(Vec<iced::widget::image::Handle>),
    Failed(String),
}

/// A press on a monster in the browser: a click if the mouse comes up where
/// it went down, a drag once it moves further than DRAG_THRESHOLD.
#[derive(Debug, Clone)]
struct Drag {
    file: PathBuf,
    monster: String,
    /// Where the cursor was first seen after the press.
    origin: Option<iced::Point>,
    /// Where it is now.
    cursor: Option<iced::Point>,
    active: bool,
}

struct Status {
    text: String,
    is_error: bool,
}

struct App {
    config: Config,
    /// Where the config file is saved.
    config_path: PathBuf,
    dialogs: Dialogs,
    sources: Vec<MonsterSource>,
    scanning: bool,
    filter: String,
    expanded: HashSet<PathBuf>,
    collapsed_sources: HashSet<String>,
    duplicate_names: HashSet<String>,
    selected_monster: Option<(PathBuf, String)>,
    /// Stat block images, by monster, as far as they've got.
    statblocks: HashMap<statblock::Key, Statblock>,
    renderer: statblock::Renderer,
    /// Where stat block images and the compiler's Python environment live.
    cache_root: PathBuf,

    docs: Vec<Document>,
    active_doc: Option<u64>,
    next_doc_id: u64,

    lhs: Option<LhsTab>,
    lhs_ratio: f32,
    rhs_ratio: f32,
    window_width: f32,
    dragging: Option<Divider>,

    config_editor: Option<ConfigEditor>,
    /// The Export to Foundry modal, while it's open.
    export: Option<export_modal::ExportDialog>,
    exporter: foundry::Exporter,
    /// The Encounter Description modal, while it's open.
    description: Option<description_modal::DescriptionDialog>,
    /// The description's window, while it's open.
    description_window: Option<description_window::DescriptionWindow>,
    describer: description::Compiler,
    /// The main window (None in headless tests, which have no windows).
    main_window: Option<window::Id>,
    /// A monster being dragged from the browser, from the press on its row.
    drag: Option<Drag>,
    /// The roster under the cursor (for dropping a dragged monster).
    drop_hover: Option<encounter::RosterRef>,
    status: Option<Status>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shortcut {
    New,
    Open,
    Save,
    SaveAll,
}

#[derive(Debug, Clone)]
enum Message {
    New,
    Open,
    Opened(Vec<PathBuf>),
    Save,
    SaveAll,
    SavePicked(u64, Option<PathBuf>),
    SelectDoc(u64),
    CloseDoc(u64),
    Shortcut(Shortcut),
    /// Tab (or Shift+Tab, `back`): focus the next (previous) text input.
    FocusMove { back: bool },

    OpenConfig,
    Config(config_modal::Msg),
    /// Opens the Export to Foundry modal for the active encounter.
    OpenExport,
    Export(export_modal::Msg),
    /// Opens the Encounter Description modal for the active encounter.
    OpenDescription,
    Description(description_modal::Msg),
    DescriptionCompiled(Result<description::Compiled, String>),
    DescriptionWindow(description_window::Msg),
    /// An event from a window: acted on only if it's the main window's.
    InWindow(window::Id, Box<Message>),
    WindowClosed(window::Id),

    ToggleLhs(LhsTab),
    DividerPressed(Divider),
    DragTo(f32),
    DragEnd,
    WindowResized(Size),

    Rescan,
    Scanned(Vec<MonsterSource>),
    Filter(String),
    ToggleFile(PathBuf),
    ToggleSource(String),
    SelectMonster(PathBuf, String),
    /// The mouse went down on a monster in the browser.
    MonsterPressed(PathBuf, String),
    /// The cursor moved while a monster is pressed (window coordinates).
    DragMoved(iced::Point),
    /// The mouse came up while a monster is pressed: a click or a drop.
    DragReleased,
    /// The cursor is over this roster.
    DropHover(encounter::RosterRef),
    /// The cursor left this roster.
    DropLeave(encounter::RosterRef),
    StatblockDrawn(statblock::Key, Result<Vec<Vec<u8>>, String>),
    RecompileStatblock,
    CloseViewer,
    DismissStatus,
    /// An edit to an open encounter, by document id.
    Edit(u64, editor::Edit),
    /// Adds the monster in the viewer to the active encounter's target roster.
    AddToRoster,
    /// Shows a reference's monster (by reference id) in the viewer.
    ShowReference(String),
    /// A monster's name clicked in a roster: picks that roster (of that
    /// document) and shows the monster.
    RosterMonster(u64, encounter::RosterRef, String),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let config_path = Config::file_path();
        let (config, error) = Config::load_from(&config_path);
        let (mut app, task) = Self::with_config(config, config_path, Dialogs::Native);
        app.status = error.map(|text| Status { text, is_error: true });
        let (id, open) = window::open(main_window_settings());
        app.main_window = Some(id);
        (app, Task::batch([task, open.then(window::size).map(Message::WindowResized)]))
    }

    /// Builds the app around a given config without reading any file.
    /// `config_path` is where the config modal saves to.
    fn with_config(config: Config, config_path: PathBuf, dialogs: Dialogs) -> (Self, Task<Message>) {
        let mut app = Self {
            config,
            config_path,
            dialogs,
            sources: Vec::new(),
            scanning: false,
            filter: String::new(),
            expanded: HashSet::new(),
            collapsed_sources: HashSet::new(),
            duplicate_names: HashSet::new(),
            selected_monster: None,
            statblocks: HashMap::new(),
            renderer: statblock::Renderer::default(),
            cache_root: statblock::default_cache_root(),
            docs: Vec::new(),
            active_doc: None,
            next_doc_id: 1,
            lhs: Some(LhsTab::Monsters),
            lhs_ratio: 0.25,
            rhs_ratio: DEFAULT_RHS_RATIO,
            window_width: INITIAL_SIZE.width,
            dragging: None,
            config_editor: None,
            export: None,
            exporter: foundry::Exporter::default(),
            description: None,
            description_window: None,
            describer: description::Compiler::default(),
            main_window: None,
            drag: None,
            drop_hover: None,
            status: None,
        };
        let scan = app.rescan();
        (app, scan)
    }

    fn title(&self) -> String {
        match self.active() {
            Some(doc) => format!("{} — Encounter Designer", doc.title()),
            None => "Encounter Designer".into(),
        }
    }

    fn active(&self) -> Option<&Document> {
        self.docs.iter().find(|d| Some(d.id) == self.active_doc)
    }

    fn rescan(&mut self) -> Task<Message> {
        self.scanning = true;
        let config = self.config.clone();
        Task::perform(async move { monsters::scan(&config) }, Message::Scanned)
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(Status { text: text.into(), is_error });
    }

    fn add_doc(&mut self, path: Option<PathBuf>, encounter: Encounter, dirty: bool) {
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.push(Document {
            id,
            path,
            encounter,
            dirty,
            target: encounter::RosterRef::FIRST,
            local_names_open: HashSet::new(),
        });
        self.active_doc = Some(id);
    }

    /// Writes a document to its path, or asks for one if it has none.
    fn save_doc(&mut self, id: u64) -> Task<Message> {
        let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) else {
            return Task::none();
        };
        match &doc.path {
            Some(path) => {
                let result = std::fs::write(path, doc.encounter.to_xml());
                let shown = path.display().to_string();
                match result {
                    Ok(()) => {
                        doc.dirty = false;
                        self.set_status(format!("Saved {shown}"), false);
                    }
                    Err(e) => self.set_status(format!("Couldn't save {shown}: {e}"), true),
                }
                Task::none()
            }
            None => {
                let name = format!("{}.xml", doc.encounter.id);
                let dir = PathBuf::from(&self.config.resource_dir);
                Task::perform(self.dialogs.save_file(dir, name, dialogs::ENCOUNTER), move |p| Message::SavePicked(id, p))
            }
        }
    }

    /// The selected monster's key in the stat block cache.
    fn selected_key(&self) -> Option<statblock::Key> {
        self.selected_monster
            .as_ref()
            .map(|(file, monster)| statblock::Key { file: file.clone(), monster: monster.clone() })
    }

    /// Shows the selected monster's stat block: from memory, from the image
    /// cache, or by compiling it (always, with `force`).
    fn show_statblock(&mut self, force: bool) -> Task<Message> {
        let Some(key) = self.selected_key() else {
            return Task::none();
        };
        match self.statblocks.get(&key) {
            Some(Statblock::Compiling) => return Task::none(),
            Some(Statblock::Ready(_)) if !force => return Task::none(),
            _ => {}
        }
        if !force
            && let Some(pages) = statblock::cached(&self.cache_root, &key)
            && let Ok(images) = statblock::load(&pages)
        {
            let handles = images.into_iter().map(iced::widget::image::Handle::from_bytes).collect();
            self.statblocks.insert(key, Statblock::Ready(handles));
            return Task::none();
        }
        self.statblocks.insert(key.clone(), Statblock::Compiling);
        let request = statblock::Request {
            key: key.clone(),
            compiler_dir: self.config.resolve(&self.config.compiler_dir),
            uv_command: self.config.uv_command.clone(),
            cache_root: self.cache_root.clone(),
        };
        Task::perform(statblock::render(self.renderer.clone(), request), move |r| {
            Message::StatblockDrawn(key.clone(), r)
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::New => self.add_doc(None, Encounter::default(), true),
            Message::Open => {
                let dir = PathBuf::from(&self.config.resource_dir);
                return Task::perform(self.dialogs.pick_files(dir), Message::Opened);
            }
            Message::Opened(paths) => {
                let mut errors = Vec::new();
                for path in paths {
                    if let Some(doc) = self.docs.iter().find(|d| d.path.as_ref() == Some(&path)) {
                        self.active_doc = Some(doc.id);
                        continue;
                    }
                    let parsed = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|t| Encounter::parse(&t));
                    match parsed {
                        Ok(enc) => self.add_doc(Some(path), enc, false),
                        Err(e) => errors.push(format!("{}: {e}", path.display())),
                    }
                }
                if !errors.is_empty() {
                    self.set_status(format!("Couldn't open {}", errors.join("; ")), true);
                }
            }
            Message::Save => {
                if let Some(id) = self.active_doc {
                    return self.save_doc(id);
                }
            }
            Message::SaveAll => {
                // Files with a path save at once; unsaved ones ask for a path
                // one after another.
                let ids: Vec<u64> = self.docs.iter().map(|d| d.id).collect();
                let mut task = Task::none();
                for id in ids {
                    task = task.chain(self.save_doc(id));
                }
                return task;
            }
            Message::SavePicked(_, None) => {}
            Message::SavePicked(id, Some(path)) => {
                if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                    doc.path = Some(path);
                    return self.save_doc(id);
                }
            }
            Message::SelectDoc(id) => {
                self.active_doc = Some(id);
                self.drop_hover = None;
            }
            Message::CloseDoc(id) => {
                if let Some(i) = self.docs.iter().position(|d| d.id == id) {
                    self.docs.remove(i);
                    if self.active_doc == Some(id) {
                        self.active_doc = self.docs.get(i.min(self.docs.len().saturating_sub(1))).map(|d| d.id);
                    }
                }
            }
            Message::Shortcut(s) => {
                if self.config_editor.is_none() && self.export.is_none() && self.description.is_none() {
                    let msg = match s {
                        Shortcut::New => Message::New,
                        Shortcut::Open => Message::Open,
                        Shortcut::Save => Message::Save,
                        Shortcut::SaveAll => Message::SaveAll,
                    };
                    return self.update(msg);
                }
            }

            Message::FocusMove { back } => {
                return if back {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::OpenConfig => {
                self.config_editor =
                    Some(ConfigEditor::new(&self.config, self.dialogs.clone(), self.config_path.clone()));
            }
            Message::Config(msg) => {
                let Some(editor) = &mut self.config_editor else {
                    return Task::none();
                };
                match editor.update(msg) {
                    config_modal::Action::None => {}
                    config_modal::Action::Run(task) => return task.map(Message::Config),
                    config_modal::Action::Close(None) => self.config_editor = None,
                    config_modal::Action::Close(Some(config)) => {
                        self.config_editor = None;
                        match config.save_to(&self.config_path) {
                            Ok(()) => self.set_status("Configuration saved", false),
                            Err(e) => self.set_status(e, true),
                        }
                        self.config = config;
                        return self.rescan();
                    }
                }
            }

            Message::OpenExport => {
                if let Some(doc) = self.active() {
                    let plan = foundry::plan(&doc.encounter, &self.config);
                    self.export = Some(export_modal::ExportDialog::new(
                        doc.encounter.id.clone(),
                        plan,
                        self.config.foundry_export_dir.clone(),
                    ));
                }
            }
            Message::Export(msg) => return self.update_export(msg),

            Message::OpenDescription => {
                if let Some(doc) = self.active() {
                    let e = &doc.encounter;
                    let rosters: Vec<_> = description::roster_options(e).into_iter().map(|r| (r, true)).collect();
                    let mut dialog = description_modal::DescriptionDialog {
                        doc: doc.id,
                        encounter: e.id.clone(),
                        rosters,
                        blocks: Vec::new(),
                        missing: Vec::new(),
                    };
                    let (blocks, missing) =
                        description::block_options(e, &dialog.chosen_rosters(), &self.config, &self.lookup());
                    dialog.set_blocks(blocks, missing);
                    self.description = Some(dialog);
                }
            }
            Message::Description(msg) => return self.update_description(msg),
            Message::DescriptionCompiled(result) => {
                if let Some(w) = &mut self.description_window {
                    w.state = match result {
                        Ok(c) => description_window::State::Ready {
                            pdf: c.pdf,
                            pages: c.pages.into_iter().map(iced::widget::image::Handle::from_bytes).collect(),
                            warnings: c.warnings,
                        },
                        Err(e) => description_window::State::Failed(e),
                    };
                }
            }
            Message::DescriptionWindow(msg) => return self.update_description_window(msg),
            Message::InWindow(id, msg) => {
                if self.main_window.is_none_or(|main| main == id) {
                    return self.update(*msg);
                }
            }
            Message::WindowClosed(id) => {
                if Some(id) == self.main_window {
                    return iced::exit();
                }
                if self.description_window.as_ref().is_some_and(|w| w.id == id) {
                    self.description_window = None;
                }
            }

            Message::ToggleLhs(tab) => {
                self.lhs = if self.lhs == Some(tab) { None } else { Some(tab) };
            }
            Message::DividerPressed(divider) => self.dragging = Some(divider),
            Message::DragTo(x) => {
                let w = self.window_width;
                match self.dragging {
                    _ if w <= 0.0 => {}
                    Some(Divider::Lhs) => self.lhs_ratio = ((x - TAB_STRIP_WIDTH) / w).clamp(0.1, 0.7),
                    Some(Divider::Rhs) => self.rhs_ratio = ((w - x) / w).clamp(0.15, 0.7),
                    None => {}
                }
            }
            Message::DragEnd => self.dragging = None,
            Message::WindowResized(size) => self.window_width = size.width,

            Message::Rescan => return self.rescan(),
            Message::Scanned(sources) => {
                self.scanning = false;
                self.duplicate_names = duplicate_names(&sources);
                self.sources = sources;
            }
            Message::Filter(v) => self.filter = v,
            Message::ToggleSource(key) => {
                if !self.collapsed_sources.remove(&key) {
                    self.collapsed_sources.insert(key);
                }
            }
            Message::ToggleFile(path) => {
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
            }
            Message::SelectMonster(path, id) => {
                self.selected_monster = Some((path, id));
                return self.show_statblock(false);
            }
            Message::RecompileStatblock => return self.show_statblock(true),
            Message::MonsterPressed(file, monster) => {
                self.drag = Some(Drag { file, monster, origin: None, cursor: None, active: false });
            }
            Message::DragMoved(p) => {
                if let Some(drag) = &mut self.drag {
                    let origin = *drag.origin.get_or_insert(p);
                    drag.cursor = Some(p);
                    if !drag.active && origin.distance(p) > DRAG_THRESHOLD {
                        drag.active = true;
                    }
                }
            }
            Message::DragReleased => {
                let Some(drag) = self.drag.take() else {
                    return Task::none();
                };
                if !drag.active {
                    // It never moved: a click.
                    return self.update(Message::SelectMonster(drag.file, drag.monster));
                }
                let Some(at) = self.drop_hover else {
                    return Task::none(); // dropped somewhere else: nothing happens
                };
                let file = self.config.to_res_path(&drag.file);
                if let Some(doc) = self.docs.iter_mut().find(|d| Some(d.id) == self.active_doc)
                    && doc.encounter.add_monster(at, &file, &drag.monster).is_some()
                {
                    doc.dirty = true;
                }
            }
            Message::DropHover(at) => self.drop_hover = Some(at),
            Message::DropLeave(at) => {
                if self.drop_hover == Some(at) {
                    self.drop_hover = None;
                }
            }
            Message::StatblockDrawn(key, result) => {
                let state = match result {
                    Ok(images) => Statblock::Ready(
                        images.into_iter().map(iced::widget::image::Handle::from_bytes).collect(),
                    ),
                    Err(e) => Statblock::Failed(e),
                };
                self.statblocks.insert(key, state);
            }
            Message::CloseViewer => self.selected_monster = None,
            Message::DismissStatus => self.status = None,
            Message::Edit(id, edit) => {
                if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                    doc.apply(edit);
                }
            }
            Message::AddToRoster => {
                let Some(key) = self.selected_key() else {
                    return Task::none();
                };
                let file = self.config.to_res_path(&key.file);
                let Some(doc) = self.docs.iter_mut().find(|d| Some(d.id) == self.active_doc) else {
                    return Task::none();
                };
                if doc.encounter.roster(doc.target).is_none() {
                    doc.target = encounter::RosterRef::FIRST;
                }
                if doc.encounter.add_monster(doc.target, &file, &key.monster).is_some() {
                    doc.dirty = true;
                }
            }
            Message::RosterMonster(doc, at, ref_id) => {
                let _ = self.update(Message::Edit(doc, editor::Edit::Target(at)));
                return self.update(Message::ShowReference(ref_id));
            }
            Message::ShowReference(ref_id) => {
                let Some(reference) =
                    self.active().and_then(|d| d.encounter.references.iter().find(|r| r.id == ref_id))
                else {
                    return Task::none();
                };
                // Prefer the scanned file's path, so the viewer and the
                // browser agree on which monster is selected.
                let path = self
                    .referenced_monster(reference)
                    .map(|(f, _)| f.path.clone())
                    .unwrap_or_else(|| self.config.resolve(&reference.file));
                self.selected_monster = Some((path, reference.monster.clone()));
                return self.show_statblock(false);
            }
        }
        Task::none()
    }

    fn update_export(&mut self, msg: export_modal::Msg) -> Task<Message> {
        use export_modal::{Msg, State};
        let Some(export) = &mut self.export else {
            return Task::none();
        };
        match msg {
            Msg::Folder(v) => export.folder = v,
            Msg::Browse => {
                let start = match export.folder.trim() {
                    "" => PathBuf::from(&self.config.resource_dir),
                    f => self.config.resolve(f),
                };
                return Task::perform(self.dialogs.pick_folder(start), |p| Message::Export(Msg::Browsed(p)));
            }
            Msg::Browsed(None) => {}
            Msg::Browsed(Some(path)) => export.folder = self.config.to_res_path(&path),
            Msg::Export => {
                if !export.can_export() {
                    return Task::none();
                }
                export.state = State::Running;
                let folder = export.folder.trim().to_string();
                let job = foundry::Job {
                    out: self.config.resolve(&folder),
                    root: PathBuf::from(&self.config.resource_dir),
                    actors: export.plan.actors.clone(),
                    compiler_dir: self.config.resolve(&self.config.compiler_dir),
                    uv_command: self.config.uv_command.clone(),
                    cache_root: self.cache_root.clone(),
                };
                // The next export starts in the same folder.
                if self.config.foundry_export_dir != folder {
                    self.config.foundry_export_dir = folder;
                    if let Err(e) = self.config.save_to(&self.config_path) {
                        self.set_status(e, true);
                    }
                }
                return Task::perform(foundry::export(self.exporter.clone(), job), |r| {
                    Message::Export(Msg::Done(r))
                });
            }
            Msg::Done(result) => {
                let folder = export.folder.trim().to_string();
                let (text, is_error) = match &result {
                    Ok(r) if r.errors.is_empty() => {
                        (format!("Exported {} Foundry actors to {folder}", r.written.len()), false)
                    }
                    Ok(r) => (
                        format!(
                            "Exported {} Foundry actors to {folder}; {} couldn't be",
                            r.written.len(),
                            r.errors.len()
                        ),
                        true,
                    ),
                    Err(_) => ("The Foundry export failed".to_string(), true),
                };
                export.state = State::Done(result);
                self.set_status(text, is_error);
            }
            Msg::Back => export.state = State::Ready,
            Msg::Close => {
                if !matches!(export.state, State::Running) {
                    self.export = None;
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        // Window events are tagged with their window: the main window's
        // layout, shortcuts and drags ignore the description window's.
        let resize = window::resize_events()
            .map(|(id, size)| Message::InWindow(id, Box::new(Message::WindowResized(size))));
        let keys = event::listen_with(|event, status, id| {
            key_message(event, status).map(|m| Message::InWindow(id, Box::new(m)))
        });
        let closed = window::close_events().map(Message::WindowClosed);
        let mut subs = vec![resize, keys, closed];
        if self.drag.is_some() {
            // Follow the pressed monster anywhere in the window, so letting go
            // off a roster (or off the body) ends the drag too.
            subs.push(event::listen_with(|event, _, id| {
                let m = match event {
                    Event::Mouse(mouse::Event::CursorMoved { position }) => Message::DragMoved(position),
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Message::DragReleased,
                    _ => return None,
                };
                Some(Message::InWindow(id, Box::new(m)))
            }));
        }
        Subscription::batch(subs)
    }

    /// The description modal's messages.
    fn update_description(&mut self, msg: description_modal::Msg) -> Task<Message> {
        use description_modal::Msg;
        let Some(dialog) = &mut self.description else {
            return Task::none();
        };
        match msg {
            Msg::Roster(i, on) => {
                if let Some(r) = dialog.rosters.get_mut(i) {
                    r.1 = on;
                }
                let rosters = dialog.chosen_rosters();
                let doc = dialog.doc;
                let (blocks, missing) = match self.docs.iter().find(|d| d.id == doc) {
                    Some(d) => description::block_options(&d.encounter, &rosters, &self.config, &self.lookup()),
                    None => Default::default(),
                };
                if let Some(dialog) = &mut self.description {
                    dialog.set_blocks(blocks, missing);
                }
            }
            Msg::Block(i, on) => {
                if let Some(b) = dialog.blocks.get_mut(i) {
                    b.1 = on;
                }
            }
            Msg::AllBlocks(on) => dialog.blocks.iter_mut().for_each(|b| b.1 = on),
            Msg::Close => self.description = None,
            Msg::Compile => {
                if !dialog.can_compile() {
                    return Task::none();
                }
                let Some(dialog) = self.description.take() else { return Task::none() };
                let Some(doc) = self.docs.iter().find(|d| d.id == dialog.doc) else {
                    return Task::none();
                };
                let e = &doc.encounter;
                let out = description::out_dir(&self.cache_root, &e.id);
                let job = description::job(
                    e,
                    &dialog.chosen_rosters(),
                    &dialog.chosen_blocks(),
                    &self.config,
                    &self.lookup(),
                    out,
                );
                let start_dir = doc
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from(&self.config.resource_dir));
                let tools = description::Tools {
                    compiler_dir: self.config.resolve(&self.config.compiler_dir),
                    uv_command: self.config.uv_command.clone(),
                    cache_root: self.cache_root.clone(),
                };
                // One description window: a new description replaces what it shows.
                let (id, show) = match &self.description_window {
                    Some(w) => (w.id, window::gain_focus(w.id)),
                    None => {
                        let (id, open) = window::open(description_window_settings());
                        (id, open.discard())
                    }
                };
                self.description_window = Some(description_window::DescriptionWindow {
                    id,
                    title: job.title.clone(),
                    start_dir,
                    state: description_window::State::Compiling,
                    saved: None,
                });
                let compile = Task::perform(
                    description::compile(self.describer.clone(), job, tools),
                    Message::DescriptionCompiled,
                );
                return Task::batch([show, compile]);
            }
        }
        Task::none()
    }

    /// The description window's messages.
    fn update_description_window(&mut self, msg: description_window::Msg) -> Task<Message> {
        use description_window::Msg;
        let Some(w) = &mut self.description_window else {
            return Task::none();
        };
        match msg {
            Msg::SavePdf => {
                if w.pdf().is_some() {
                    let name = format!("{}.pdf", w.title);
                    let ask = self.dialogs.save_file(w.start_dir.clone(), name, dialogs::PDF);
                    return Task::perform(ask, |p| Message::DescriptionWindow(Msg::SaveTo(p)));
                }
            }
            Msg::SaveTo(None) => {}
            Msg::SaveTo(Some(path)) => {
                let path = if path.extension().is_none() { path.with_extension("pdf") } else { path };
                if let Some(pdf) = w.pdf() {
                    w.saved = Some(match std::fs::copy(pdf, &path) {
                        Ok(_) => Ok(path),
                        Err(e) => Err(format!("Couldn't save {}: {e}", path.display())),
                    });
                }
            }
        }
        Task::none()
    }

    /// The monster files' name and CR for a reference's stat block.
    fn lookup(&self) -> impl Fn(&encounter::Reference) -> Option<(String, Option<String>)> + '_ {
        |r| self.referenced_monster(r).map(|(_, m)| (m.name.clone(), m.cr.clone()))
    }

    // ----- views -------------------------------------------------------------

    fn lhs_width(&self) -> f32 {
        (self.lhs_ratio * self.window_width).max(120.0)
    }

    /// The viewer's width: its share of the window, but giving way so the
    /// encounter editor keeps at least MIN_MAIN_WIDTH.
    fn rhs_width(&self) -> f32 {
        let lhs = if self.lhs.is_some() { self.lhs_width() + DIVIDER_WIDTH } else { 0.0 };
        let room = self.window_width - TAB_STRIP_WIDTH - lhs - DIVIDER_WIDTH - MIN_MAIN_WIDTH;
        (self.rhs_ratio * self.window_width).min(room).max(200.0)
    }

    /// A window's view: the description's window, or the main window.
    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        match &self.description_window {
            Some(w) if w.id == id => w.view().map(Message::DescriptionWindow),
            _ => self.view(),
        }
    }

    fn window_title(&self, id: window::Id) -> String {
        match &self.description_window {
            Some(w) if w.id == id => format!("{} — Encounter Description", w.title),
            _ => self.title(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut body = row![self.tab_strip()];
        if let Some(tab) = self.lhs {
            let width = self.lhs_width();
            let panel = match tab {
                LhsTab::Monsters => self.monster_browser(),
            };
            body = body
                .push(container(panel).width(width).height(Length::Fill).padding(8))
                .push(
                    mouse_area(
                        container(rule::vertical(1))
                            .width(DIVIDER_WIDTH)
                            .height(Length::Fill)
                            .align_x(Alignment::Center),
                    )
                    .interaction(mouse::Interaction::ResizingHorizontally)
                    .on_press(Message::DividerPressed(Divider::Lhs)),
                );
        }
        body = body.push(self.documents());
        if self.selected_monster.is_some() {
            let width = self.rhs_width();
            body = body
                .push(
                    mouse_area(
                        container(rule::vertical(1))
                            .width(DIVIDER_WIDTH)
                            .height(Length::Fill)
                            .align_x(Alignment::Center),
                    )
                    .interaction(mouse::Interaction::ResizingHorizontally)
                    .on_press(Message::DividerPressed(Divider::Rhs)),
                )
                .push(container(self.monster_viewer()).width(width).height(Length::Fill).padding(8));
        }

        // The body tracks the cursor while the divider is dragged. It is
        // always wrapped, so starting a drag doesn't rebuild the tree (and
        // lose scroll positions).
        let mut body = mouse_area(body.height(Length::Fill));
        if self.dragging.is_some() {
            body = body
                .on_move(|p| Message::DragTo(p.x))
                .on_release(Message::DragEnd)
                .interaction(mouse::Interaction::ResizingHorizontally);
        }

        let mut page = column![self.ribbon(), rule::horizontal(1), body];
        if let Some(status) = &self.status {
            page = page.push(self.status_bar(status));
        }
        // The page is always the first layer of the same stack, with the
        // dragged monster and any modal over it (or empty layers), so
        // starting a drag or opening a modal doesn't rebuild the page's
        // widget tree and lose its scroll positions.
        let ghost: Element<_> = match &self.drag {
            Some(Drag { active: true, cursor: Some(p), file, monster, .. }) => {
                let name = self
                    .sources
                    .iter()
                    .flat_map(|s| &s.files)
                    .filter(|f| &f.path == file)
                    .flat_map(|f| &f.monsters)
                    .find(|m| &m.id == monster)
                    .map(|m| m.label())
                    .unwrap_or_else(|| monster.clone());
                let hint = if self.drop_hover.is_some() { "Release to add" } else { "Drop on a roster" };
                let ghost = container(column![text(name).size(13), text(hint).size(11).style(text::secondary)])
                    .padding([4, 10])
                    .style(container::bordered_box);
                iced::widget::pin(ghost).position(*p + iced::Vector::new(14.0, 10.0)).into()
            }
            _ => space().into(),
        };
        let modal: Element<_> = match (&self.config_editor, &self.export, &self.description) {
            (Some(editor), _, _) => ui::modal_layer(editor.view().map(Message::Config)),
            (None, Some(export), _) => ui::modal_layer(export.view().map(Message::Export)),
            (None, None, Some(d)) => ui::modal_layer(d.view().map(Message::Description)),
            (None, None, None) => space().into(),
        };
        iced::widget::stack![page, ghost, modal].into()
    }

    fn ribbon(&self) -> Element<'_, Message> {
        let has_doc = self.active_doc.is_some();
        let btn = |label, msg: Option<Message>| {
            button(text(label).size(14)).padding([6, 12]).style(button::text).on_press_maybe(msg)
        };
        container(
            row![
                btn("New", Some(Message::New)),
                btn("Open", Some(Message::Open)),
                btn("Save", has_doc.then_some(Message::Save)),
                btn("Save All", (!self.docs.is_empty()).then_some(Message::SaveAll)),
                rule::vertical(1),
                btn("Encounter Description", has_doc.then_some(Message::OpenDescription)),
                btn("Export to Foundry", has_doc.then_some(Message::OpenExport)),
                rule::vertical(1),
                btn("Configuration", Some(Message::OpenConfig)),
            ]
            .spacing(4)
            .height(36)
            .align_y(Alignment::Center),
        )
        .padding([2, 6])
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
    }

    fn tab_strip(&self) -> Element<'_, Message> {
        let tabs = LhsTab::ALL.iter().map(|&tab| {
            // iced can't rotate text, so the label runs top to bottom.
            let label: String = tab.label().to_uppercase().chars().map(|c| format!("{c}\n")).collect();
            button(text(label.trim_end().to_string()).size(12).align_x(Alignment::Center).width(Length::Fill))
                .width(TAB_STRIP_WIDTH)
                .padding([8, 0])
                .style(if self.lhs == Some(tab) { button::primary } else { button::text })
                .on_press(Message::ToggleLhs(tab))
                .into()
        });
        container(column(tabs).spacing(4))
            .width(TAB_STRIP_WIDTH)
            .height(Length::Fill)
            .style(container::rounded_box)
            .into()
    }

    fn monster_browser(&self) -> Element<'_, Message> {
        let header = row![
            text("Monsters").size(18),
            space::horizontal(),
            button(text(if self.scanning { "Scanning…" } else { "Rescan" }).size(12))
                .style(button::secondary)
                .on_press_maybe((!self.scanning).then_some(Message::Rescan)),
        ]
        .align_y(Alignment::Center);

        let filter = text_input("Filter monsters…", &self.filter)
            .id(MONSTER_FILTER_INPUT)
            .on_input(Message::Filter);
        let needle = self.filter.trim().to_lowercase();
        let filtering = !needle.is_empty();
        let matches = |s: &str| !filtering || s.to_lowercase().contains(&needle);

        let mut list = column![].spacing(1);
        if self.config.srd_monster_dir.path.trim().is_empty() {
            list = list.push(
                text("No SRD monster folder set. Add one in Configuration.")
                    .size(12)
                    .style(text::secondary),
            );
        }

        for source in &self.sources {
            // What this folder shows: single-monster files as plain rows,
            // multi-monster files as headings over their monsters.
            enum Item<'a> {
                Single(&'a monsters::MonsterFile, &'a monsters::MonsterEntry),
                Group(&'a monsters::MonsterFile, Vec<&'a monsters::MonsterEntry>),
                Broken(&'a monsters::MonsterFile),
            }
            let mut items: Vec<(String, Item)> = Vec::new();
            for file in &source.files {
                if file.error.is_some() {
                    if matches(&file.relative) {
                        items.push((file.relative.to_lowercase(), Item::Broken(file)));
                    }
                    continue;
                }
                match file.monsters.as_slice() {
                    [] => {}
                    [m] => {
                        if matches(&m.name) || matches(&m.id) {
                            items.push((m.name.to_lowercase(), Item::Single(file, m)));
                        }
                    }
                    all => {
                        let heading = file.family.clone().unwrap_or_else(|| file.relative.clone());
                        let whole = matches(&heading);
                        let shown: Vec<_> = all
                            .iter()
                            .filter(|m| whole || matches(&m.name) || matches(&m.id))
                            .collect();
                        if !shown.is_empty() {
                            items.push((heading.to_lowercase(), Item::Group(file, shown)));
                        }
                    }
                }
            }
            items.sort_by(|a, b| a.0.cmp(&b.0));
            let count: usize = items
                .iter()
                .map(|(_, i)| match i {
                    Item::Single(..) => 1,
                    Item::Group(_, ms) => ms.len(),
                    Item::Broken(_) => 0,
                })
                .sum();

            let key = source_key(source);
            let open = !self.collapsed_sources.contains(&key);
            let label = if source.is_srd {
                format!("SRD · {}", source.dir.display())
            } else {
                source.dir.display()
            };
            list = list.push(
                button(text(format!("{} {label}  ({count})", arrow(open))).size(13))
                    .width(Length::Fill)
                    .padding([5, 4])
                    .style(button::secondary)
                    .on_press(Message::ToggleSource(key)),
            );
            if !open {
                continue;
            }
            if let Some(e) = &source.error {
                list = list.push(indent(text(e).size(12).style(text::danger), 1));
                continue;
            }
            if items.is_empty() {
                let note = if filtering { "No matches" } else { "No monster files" };
                list = list.push(indent(text(note).size(12).style(text::secondary), 1));
                continue;
            }

            for (_, item) in items {
                match item {
                    Item::Single(file, m) => list = list.push(self.monster_row(file, m, 1)),
                    Item::Broken(file) => {
                        list = list.push(indent(
                            column![
                                text(&file.relative).size(13),
                                text(file.error.as_deref().unwrap_or_default())
                                    .size(11)
                                    .style(text::danger),
                            ],
                            1,
                        ));
                    }
                    Item::Group(file, monsters) => {
                        let open = filtering || self.expanded.contains(&file.path);
                        let heading = file.family.clone().unwrap_or_else(|| file.relative.clone());
                        list = list.push(indent(
                            button(
                                text(format!("{} {heading}  ({})", arrow(open), file.monsters.len()))
                                    .size(13),
                            )
                            .width(Length::Fill)
                            .padding([3, 6])
                            .style(button::text)
                            .on_press(Message::ToggleFile(file.path.clone())),
                            1,
                        ));
                        if open {
                            for m in monsters {
                                list = list.push(self.monster_row(file, m, 2));
                            }
                        }
                    }
                }
            }
        }

        column![header, filter, scrollable(list).height(Length::Fill)]
            .spacing(8)
            .into()
    }

    /// One monster: "Name (CR X)", plus its file when another file has a
    /// monster of the same name.
    fn monster_row<'a>(
        &self,
        file: &'a monsters::MonsterFile,
        m: &'a monsters::MonsterEntry,
        depth: u16,
    ) -> Element<'a, Message> {
        let selected = self
            .selected_monster
            .as_ref()
            .is_some_and(|(p, id)| p == &file.path && id == &m.id);
        let mut content = column![text(m.label()).size(13)];
        if self.duplicate_names.contains(&m.name.to_lowercase()) {
            content = content.push(text(&file.relative).size(11).style(if selected {
                text::base
            } else {
                text::secondary
            }));
        }
        // Not a button: a press can become a drag onto a roster, and a click
        // (press and release in place) selects the monster.
        let row = container(content).width(Length::Fill).padding([2, 6]).style(move |theme: &iced::Theme| {
            if selected {
                let p = theme.extended_palette().primary.base;
                container::Style::default().background(p.color).color(p.text)
            } else {
                container::Style::default()
            }
        });
        indent(
            mouse_area(row)
                .interaction(mouse::Interaction::Pointer)
                .on_press(Message::MonsterPressed(file.path.clone(), m.id.clone()))
                .on_release(Message::DragReleased),
            depth,
        )
    }

    /// The selected monster's stat block, as the compiler draws it.
    fn monster_viewer(&self) -> Element<'_, Message> {
        let Some(key) = self.selected_key() else {
            return space().into();
        };
        let title = self
            .sources
            .iter()
            .flat_map(|s| &s.files)
            .filter(|f| f.path == key.file)
            .flat_map(|f| &f.monsters)
            .find(|m| m.id == key.monster)
            .map(|m| m.label())
            .unwrap_or_else(|| key.monster.clone());
        let state = self.statblocks.get(&key);
        let compiling = matches!(state, Some(Statblock::Compiling));

        // The title on its own line and the buttons wrapping under it: a
        // narrow panel squeezed a single row until the buttons drew outside
        // their (zero-width) clickable area.
        let header = column![
            text(title).size(18),
            row![
                button(text("Add to roster").size(12))
                    .on_press_maybe(self.active_doc.is_some().then_some(Message::AddToRoster)),
                button(text("Recompile").size(12))
                    .style(button::secondary)
                    .on_press_maybe((!compiling).then_some(Message::RecompileStatblock)),
                button(text("Close").size(12)).style(button::secondary).on_press(Message::CloseViewer),
            ]
            .spacing(6)
            .wrap(),
        ]
        .spacing(4);

        let body: Element<_> = match state {
            None | Some(Statblock::Compiling) => column![
                text("Compiling stat block…").style(text::secondary),
                text("The first one sets up the compiler's Python environment and can take a minute.")
                    .size(12)
                    .style(text::secondary),
            ]
            .spacing(4)
            .into(),
            Some(Statblock::Failed(e)) => column![
                text("Couldn't draw this stat block").style(text::danger),
                text(e.as_str()).size(12),
            ]
            .spacing(4)
            .into(),
            Some(Statblock::Ready(images)) => scrollable(
                column(images.iter().map(|h| iced::widget::image(h.clone()).width(Length::Fill).into()))
                    .spacing(0), // column pieces of one stat block: no gap
            )
            .height(Length::Fill)
            .into(),
        };

        column![header, rule::horizontal(1), body].spacing(8).into()
    }

    fn documents(&self) -> Element<'_, Message> {
        if self.docs.is_empty() {
            return container(
                text("No encounters open. Use New or Open on the ribbon.").style(text::secondary),
            )
            .center(Length::Fill)
            .into();
        }

        let tabs = row(self.docs.iter().map(|doc| {
            let active = self.active_doc == Some(doc.id);
            row![
                button(text(doc.title()).size(13))
                    .padding([5, 10])
                    .style(if active { button::primary } else { button::secondary })
                    .on_press(Message::SelectDoc(doc.id)),
                button(text("×").size(13))
                    .padding([5, 8])
                    .style(button::text)
                    .on_press(Message::CloseDoc(doc.id)),
            ]
            .into()
        }))
        .spacing(4)
        .wrap();

        let body: Element<_> = match self.active() {
            Some(doc) => self.encounter_editor(doc),
            None => space().into(),
        };

        column![tabs, rule::horizontal(1), body]
            .spacing(6)
            .padding(8)
            .width(Length::Fill)
            .into()
    }

    fn status_bar<'a>(&self, status: &'a Status) -> Element<'a, Message> {
        container(
            row![
                text(&status.text)
                    .size(12)
                    .style(if status.is_error { text::danger } else { text::secondary }),
                space::horizontal(),
                button(text("×").size(12)).style(button::text).padding([0, 6]).on_press(Message::DismissStatus),
            ]
            .align_y(Alignment::Center),
        )
        .padding([2, 8])
        .width(Length::Fill)
        .into()
    }
}

/// What a key press does: a shortcut, or Tab moving focus between text
/// inputs (which leave Tab alone, so it arrives ignored).
fn key_message(event: Event, status: event::Status) -> Option<Message> {
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Tab),
            modifiers,
            ..
        }) => (status == event::Status::Ignored).then_some(Message::FocusMove { back: modifiers.shift() }),
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            shortcut_for(&key, modifiers).map(Message::Shortcut)
        }
        _ => None,
    }
}

/// The ribbon command a key press stands for, if any. Kept apart from the
/// subscription so it can be tested.
fn shortcut_for(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Shortcut> {
    if !modifiers.command() {
        return None;
    }
    match key.as_ref() {
        keyboard::Key::Character(c) if c.eq_ignore_ascii_case("n") => Some(Shortcut::New),
        keyboard::Key::Character(c) if c.eq_ignore_ascii_case("o") => Some(Shortcut::Open),
        keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") => {
            Some(if modifiers.shift() { Shortcut::SaveAll } else { Shortcut::Save })
        }
        _ => None,
    }
}

fn arrow(open: bool) -> &'static str {
    if open { "▾" } else { "▸" }
}

fn indent<'a>(content: impl Into<Element<'a, Message>>, depth: u16) -> Element<'a, Message> {
    container(content)
        .padding(iced::Padding::ZERO.left(14.0 * f32::from(depth)))
        .into()
}

/// Identifies a monster folder in the browser's collapsed set.
fn source_key(source: &MonsterSource) -> String {
    format!("{}|{}", source.is_srd, source.dir.path)
}

/// Monster names (lowercased) found in more than one file.
fn duplicate_names(sources: &[MonsterSource]) -> HashSet<String> {
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    let mut dupes = HashSet::new();
    for file in sources.iter().flat_map(|s| &s.files) {
        for m in &file.monsters {
            let name = m.name.to_lowercase();
            match seen.get(&name) {
                Some(path) if path != &file.path => {
                    dupes.insert(name);
                }
                Some(_) => {}
                None => {
                    seen.insert(name, file.path.clone());
                }
            }
        }
    }
    dupes
}

#[cfg(test)]
mod tests {
    use super::*;
    use monsters::{MonsterEntry, MonsterFile};

    fn file(path: &str, names: &[&str]) -> MonsterFile {
        MonsterFile {
            path: PathBuf::from(path),
            relative: path.into(),
            family: None,
            monsters: names
                .iter()
                .map(|n| MonsterEntry { id: n.to_string(), name: n.to_string(), cr: None })
                .collect(),
            error: None,
        }
    }

    #[test]
    fn the_icon_is_built_in() {
        assert!(window_icon().is_some(), "assets/icon.png didn't decode");
        let ico = include_bytes!("../assets/icon.ico");
        assert_eq!(&ico[..4], &[0, 0, 1, 0]); // an .ico file
        let count = u16::from_le_bytes([ico[4], ico[5]]);
        assert_eq!(count, 7, "16, 24, 32, 48, 64, 128 and 256 px");
    }

    #[test]
    fn duplicates_are_names_in_more_than_one_file() {
        let source = |files| MonsterSource {
            dir: config::MonsterDir::default(),
            is_srd: false,
            files,
            error: None,
        };
        let sources = [
            source(vec![file("a.xml", &["Bandit", "Scout"]), file("b.xml", &["Ogre"])]),
            source(vec![file("c.xml", &["bandit", "Wolf", "Wolf"])]),
        ];
        let dupes = duplicate_names(&sources);
        assert_eq!(dupes, HashSet::from(["bandit".to_string()]));
    }
}
