//! The configuration modal: the `res://` folder, the SRD monster folder and the
//! list of further monster folders, edited as a draft and saved together.

use crate::config::{Config, MonsterDir};
use crate::dialogs::Dialogs;
use crate::ui::modal;
use iced::widget::{
    button, center, checkbox, column, container, mouse_area, row, rule, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Task};
use std::path::PathBuf;

pub const DIR_PATH_INPUT: &str = "dir-editor-path";
pub const RESOURCE_DIR_INPUT: &str = "config-resource-dir";
pub const SRD_DIR_INPUT: &str = "config-srd-dir";
pub const COMPILER_DIR_INPUT: &str = "config-compiler-dir";

pub struct ConfigEditor {
    draft: Config,
    dialogs: Dialogs,
    /// Where the config file lives, shown in the footer.
    file_path: PathBuf,
    selected: Option<usize>,
    dir_editor: Option<DirEditor>,
}

/// The add/edit modal for one monster folder.
struct DirEditor {
    /// `None` when adding.
    index: Option<usize>,
    path: String,
    include_subdirs: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum BrowseTarget {
    ResourceDir,
    SrdDir,
    DirEditor,
    CompilerDir,
}

#[derive(Debug, Clone)]
pub enum Msg {
    ResourceDir(String),
    SrdPath(String),
    SrdSubdirs(bool),
    CompilerDir(String),
    Browse(BrowseTarget),
    Browsed(BrowseTarget, Option<PathBuf>),
    Select(usize),
    New,
    Edit,
    EditAt(usize),
    Delete,
    MoveUp,
    MoveDown,
    EditorPath(String),
    EditorSubdirs(bool),
    EditorOk,
    EditorCancel,
    Defaults,
    Save,
    Cancel,
}

pub enum Action {
    None,
    Run(Task<Msg>),
    /// Close the modal; `Some` carries the config to save and apply.
    Close(Option<Config>),
}

impl ConfigEditor {
    pub fn new(config: &Config, dialogs: Dialogs, file_path: PathBuf) -> Self {
        Self { draft: config.clone(), dialogs, file_path, selected: None, dir_editor: None }
    }

    pub fn update(&mut self, msg: Msg) -> Action {
        let dirs = &mut self.draft.monster_dirs;
        match msg {
            Msg::ResourceDir(v) => self.draft.resource_dir = v,
            Msg::SrdPath(v) => self.draft.srd_monster_dir.path = v,
            Msg::SrdSubdirs(v) => self.draft.srd_monster_dir.include_subdirs = v,
            Msg::CompilerDir(v) => self.draft.compiler_dir = v,
            Msg::Browse(target) => {
                let start = match target {
                    BrowseTarget::ResourceDir => PathBuf::from(&self.draft.resource_dir),
                    BrowseTarget::SrdDir => self.draft.resolve(&self.draft.srd_monster_dir.path),
                    BrowseTarget::CompilerDir => self.draft.resolve(&self.draft.compiler_dir),
                    BrowseTarget::DirEditor => self
                        .dir_editor
                        .as_ref()
                        .map(|e| self.draft.resolve(&e.path))
                        .unwrap_or_default(),
                };
                let start = if start.is_dir() { start } else { PathBuf::from(&self.draft.resource_dir) };
                return Action::Run(Task::perform(
                    self.dialogs.pick_folder(start),
                    move |p| Msg::Browsed(target, p),
                ));
            }
            Msg::Browsed(_, None) => {}
            Msg::Browsed(target, Some(path)) => match target {
                BrowseTarget::ResourceDir => {
                    self.draft.resource_dir = path.to_string_lossy().replace('\\', "/");
                }
                BrowseTarget::SrdDir => self.draft.srd_monster_dir.path = self.draft.to_res_path(&path),
                BrowseTarget::CompilerDir => self.draft.compiler_dir = self.draft.to_res_path(&path),
                BrowseTarget::DirEditor => {
                    let res = self.draft.to_res_path(&path);
                    if let Some(e) = &mut self.dir_editor {
                        e.path = res;
                    }
                }
            },
            Msg::Select(i) => self.selected = Some(i),
            Msg::New => {
                self.dir_editor = Some(DirEditor { index: None, path: String::new(), include_subdirs: true });
                return Action::Run(iced::widget::operation::focus(DIR_PATH_INPUT));
            }
            Msg::Edit => {
                if let Some(i) = self.selected {
                    return self.update(Msg::EditAt(i));
                }
            }
            Msg::EditAt(i) => {
                if let Some(d) = dirs.get(i) {
                    self.selected = Some(i);
                    self.dir_editor = Some(DirEditor {
                        index: Some(i),
                        path: d.path.clone(),
                        include_subdirs: d.include_subdirs,
                    });
                    return Action::Run(iced::widget::operation::focus(DIR_PATH_INPUT));
                }
            }
            Msg::Delete => {
                if let Some(i) = self.selected.filter(|&i| i < dirs.len()) {
                    dirs.remove(i);
                    self.selected = if dirs.is_empty() { None } else { Some(i.min(dirs.len() - 1)) };
                }
            }
            Msg::MoveUp => {
                if let Some(i) = self.selected.filter(|&i| i > 0 && i < dirs.len()) {
                    dirs.swap(i, i - 1);
                    self.selected = Some(i - 1);
                }
            }
            Msg::MoveDown => {
                if let Some(i) = self.selected.filter(|&i| i + 1 < dirs.len()) {
                    dirs.swap(i, i + 1);
                    self.selected = Some(i + 1);
                }
            }
            Msg::EditorPath(v) => {
                if let Some(e) = &mut self.dir_editor {
                    e.path = v;
                }
            }
            Msg::EditorSubdirs(v) => {
                if let Some(e) = &mut self.dir_editor {
                    e.include_subdirs = v;
                }
            }
            Msg::EditorOk => {
                if let Some(e) = self.dir_editor.take() {
                    let path = e.path.trim().to_string();
                    if path.is_empty() {
                        self.dir_editor = Some(e);
                        return Action::None;
                    }
                    let dir = MonsterDir { path, include_subdirs: e.include_subdirs };
                    match e.index {
                        Some(i) if i < dirs.len() => dirs[i] = dir,
                        _ => {
                            dirs.push(dir);
                            self.selected = Some(dirs.len() - 1);
                        }
                    }
                }
            }
            Msg::EditorCancel => self.dir_editor = None,
            Msg::Defaults => {
                self.draft = Config::default();
                self.selected = None;
            }
            Msg::Save => return Action::Close(Some(self.draft.clone())),
            Msg::Cancel => return Action::Close(None),
        }
        Action::None
    }

    /// The modal's content; the add/edit folder modal sits over it when open.
    pub fn view(&self) -> Element<'_, Msg> {
        match &self.dir_editor {
            Some(editor) => modal(center(self.main_panel()), self.dir_editor_panel(editor)),
            None => self.main_panel(),
        }
    }

    fn main_panel(&self) -> Element<'_, Msg> {
        let d = &self.draft;
        let missing = |path: &str| -> Element<'_, Msg> {
            if path.trim().is_empty() || d.resolve(path).is_dir() {
                space().into()
            } else {
                text("Folder not found").size(12).style(text::danger).into()
            }
        };

        let resource = column![
            text("Resource folder (res://)").size(14),
            row![
                text_input("e:/Data/enigma-narrative/", &d.resource_dir)
                    .id(RESOURCE_DIR_INPUT)
                    .on_input(Msg::ResourceDir),
                button("Browse…").on_press(Msg::Browse(BrowseTarget::ResourceDir)),
            ]
            .spacing(6),
            missing(&d.resource_dir),
        ]
        .spacing(4);

        let srd = column![
            text("SRD monster folder").size(14),
            row![
                text_input("res://… or an absolute path (empty for none)", &d.srd_monster_dir.path)
                    .id(SRD_DIR_INPUT)
                    .on_input(Msg::SrdPath),
                button("Browse…").on_press(Msg::Browse(BrowseTarget::SrdDir)),
            ]
            .spacing(6),
            row![
                checkbox(d.srd_monster_dir.include_subdirs)
                    .label("Include subdirectories")
                    .on_toggle(Msg::SrdSubdirs),
                space::horizontal(),
                missing(&d.srd_monster_dir.path),
            ],
        ]
        .spacing(4);

        let list = column(d.monster_dirs.iter().enumerate().map(|(i, dir)| {
            let selected = self.selected == Some(i);
            let found = d.resolve(&dir.path).is_dir();
            let label = row![
                text(dir.display()).width(Length::Fill),
                if found { text("") } else { text("not found").size(12).style(text::danger) },
            ];
            // A plain container, not a button: a button would swallow the
            // clicks before the mouse area could see the double-click.
            let item = container(label)
                .width(Length::Fill)
                .padding([3, 6])
                .style(move |theme: &iced::Theme| {
                    if selected {
                        let p = theme.extended_palette().primary.base;
                        container::Style::default().background(p.color).color(p.text)
                    } else {
                        container::Style::default()
                    }
                });
            mouse_area(item)
                .on_press(Msg::Select(i))
                .on_double_click(Msg::EditAt(i))
                .into()
        }));
        let list_box = container(scrollable(list).height(Length::Fill))
            .padding(2)
            .height(180)
            .width(Length::Fill)
            .style(container::bordered_box);

        let has_sel = self.selected.is_some_and(|i| i < d.monster_dirs.len());
        let side = |label, msg: Msg, enabled: bool| {
            button(text(label).align_x(Alignment::Center).width(Length::Fill))
                .width(112)
                .on_press_maybe(enabled.then_some(msg))
        };
        let controls = column![
            side("New…", Msg::New, true),
            side("Edit…", Msg::Edit, has_sel),
            side("Delete", Msg::Delete, has_sel),
            side("Move up", Msg::MoveUp, self.selected.is_some_and(|i| i > 0 && has_sel)),
            side("Move down", Msg::MoveDown, self.selected.is_some_and(|i| i + 1 < d.monster_dirs.len())),
        ]
        .spacing(6);

        let dirs = column![
            text("Monster folders").size(14),
            row![list_box, controls].spacing(8),
            text("* includes subdirectories · double-click a folder to edit it")
                .size(12)
                .style(text::secondary),
        ]
        .spacing(4);

        let footer = row![
            text(self.file_path.display().to_string()).size(11).style(text::secondary),
            space::horizontal(),
            button("Restore defaults").style(button::secondary).on_press(Msg::Defaults),
            button("Cancel").style(button::secondary).on_press(Msg::Cancel),
            button("Save").on_press(Msg::Save),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let compiler = column![
            text("Stat block compiler (encompile) folder").size(14),
            row![
                text_input("res://compiler", &d.compiler_dir)
                    .id(COMPILER_DIR_INPUT)
                    .on_input(Msg::CompilerDir),
                button("Browse…").on_press(Msg::Browse(BrowseTarget::CompilerDir)),
            ]
            .spacing(6),
            row![
                text("Draws the stat blocks in the monster viewer. Needs uv and LaTeX with the dnd template.")
                    .size(12)
                    .style(text::secondary),
                space::horizontal(),
                missing(&d.compiler_dir),
            ],
        ]
        .spacing(4);

        panel(
            column![
                text("Configuration").size(20),
                resource,
                srd,
                dirs,
                compiler,
                rule::horizontal(1),
                footer
            ]
            .spacing(14),
            640,
        )
    }

    fn dir_editor_panel<'a>(&'a self, e: &'a DirEditor) -> Element<'a, Msg> {
        let title = if e.index.is_some() { "Edit monster folder" } else { "New monster folder" };
        let ok = (!e.path.trim().is_empty()).then_some(Msg::EditorOk);
        panel(
            column![
                text(title).size(18),
                row![
                    text_input("res://monsters or an absolute path", &e.path)
                        .id(DIR_PATH_INPUT)
                        .on_input(Msg::EditorPath)
                        .on_submit_maybe(ok.clone()),
                    button("Browse…").on_press(Msg::Browse(BrowseTarget::DirEditor)),
                ]
                .spacing(6),
                checkbox(e.include_subdirs)
                    .label("Include subdirectories")
                    .on_toggle(Msg::EditorSubdirs),
                row![
                    space::horizontal(),
                    button("Cancel").style(button::secondary).on_press(Msg::EditorCancel),
                    button("OK").on_press_maybe(ok),
                ]
                .spacing(8),
            ]
            .spacing(12),
            480,
        )
    }
}

fn panel<'a>(content: impl Into<Element<'a, Msg>>, width: u32) -> Element<'a, Msg> {
    container(content)
        .padding(20)
        .width(width as f32)
        .style(container::rounded_box)
        .into()
}
