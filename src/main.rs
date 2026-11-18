mod config;
mod config_modal;
mod dialogs;
mod editor;
mod encounter;
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
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .window(window::Settings {
            size: INITIAL_SIZE,
            icon: window_icon(),
            ..window::Settings::default()
        })
        .run()
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

    OpenConfig,
    Config(config_modal::Msg),

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
        (app, task)
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
            drag: None,
            drop_hover: None,
            status: None,
        };
        let size = window::latest()
            .and_then(window::size)
            .map(Message::WindowResized);
        let scan = app.rescan();
        (app, Task::batch([size, scan]))
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
        self.docs.push(Document { id, path, encounter, dirty, target: encounter::RosterRef::FIRST });
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
                Task::perform(self.dialogs.save_file(dir, name), move |p| Message::SavePicked(id, p))
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
                if self.config_editor.is_none() {
                    let msg = match s {
                        Shortcut::New => Message::New,
                        Shortcut::Open => Message::Open,
                        Shortcut::Save => Message::Save,
                        Shortcut::SaveAll => Message::SaveAll,
                    };
                    return self.update(msg);
                }
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

    fn subscription(&self) -> Subscription<Message> {
        let resize = window::resize_events().map(|(_, size)| Message::WindowResized(size));
        let keys = event::listen_with(|event, _, _| match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                shortcut_for(&key, modifiers).map(Message::Shortcut)
            }
            _ => None,
        });
        let mut subs = vec![resize, keys];
        if self.drag.is_some() {
            // Follow the pressed monster anywhere in the window, so letting go
            // off a roster (or off the body) ends the drag too.
            subs.push(event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Message::DragMoved(position)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::DragReleased),
                _ => None,
            }));
        }
        Subscription::batch(subs)
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
        let mut page: Element<_> = page.into();

        // The dragged monster's name, following the cursor.
        if let Some(Drag { active: true, cursor: Some(p), file, monster, .. }) = &self.drag {
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
            page = iced::widget::stack![page, iced::widget::pin(ghost).position(*p + iced::Vector::new(14.0, 10.0))].into();
        }

        match &self.config_editor {
            Some(editor) => ui::modal(page, editor.view().map(Message::Config)),
            None => page,
        }
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
