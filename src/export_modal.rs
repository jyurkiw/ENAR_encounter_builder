//! The Export to Foundry modal: what will be written, the folder to write
//! it to, and then what was written.

use crate::foundry::{Plan, Report};
use iced::widget::{button, column, container, row, rule, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

pub const FOLDER_INPUT: &str = "foundry-folder";

pub struct ExportDialog {
    /// The encounter's id, for the heading.
    pub encounter: String,
    pub plan: Plan,
    /// The folder to write to, as typed (absolute or `res://`).
    pub folder: String,
    pub state: State,
}

pub enum State {
    /// Choosing the folder.
    Ready,
    Running,
    Done(Result<Report, String>),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Folder(String),
    Browse,
    Browsed(Option<PathBuf>),
    /// Okay: write the files.
    Export,
    Done(Result<Report, String>),
    /// Back to choosing the folder, after a failed export.
    Back,
    Close,
}

impl ExportDialog {
    pub fn new(encounter: String, plan: Plan, folder: String) -> Self {
        ExportDialog { encounter, plan, folder, state: State::Ready }
    }

    /// Whether Okay can be pressed.
    pub fn can_export(&self) -> bool {
        matches!(self.state, State::Ready) && !self.folder.trim().is_empty() && !self.plan.actors.is_empty()
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match &self.state {
            State::Ready | State::Running => self.choose(),
            State::Done(Ok(report)) => self.report(report),
            State::Done(Err(e)) => column![
                text("The export failed").size(14),
                text(e.clone()).size(12).style(text::danger),
            ]
            .spacing(6)
            .into(),
        };
        let footer = match &self.state {
            State::Ready => row![
                space::horizontal(),
                button("Cancel").style(button::secondary).on_press(Msg::Close),
                button("Okay").on_press_maybe(self.can_export().then_some(Msg::Export)),
            ],
            State::Running => row![text("Exporting…").size(13), space::horizontal()],
            State::Done(Ok(_)) => row![space::horizontal(), button("Close").on_press(Msg::Close)],
            State::Done(Err(_)) => row![
                space::horizontal(),
                button("Back").style(button::secondary).on_press(Msg::Back),
                button("Close").on_press(Msg::Close),
            ],
        }
        .spacing(8)
        .align_y(Alignment::Center);

        container(
            column![text("Export to Foundry").size(20), body, rule::horizontal(1), footer].spacing(14),
        )
        .padding(20)
        .width(600)
        .style(container::rounded_box)
        .into()
    }

    fn choose(&self) -> Element<'_, Msg> {
        let n = self.plan.actors.len();
        let intro = text(format!(
            "{n} actor file{} for {}, for Foundry's Import Data (dnd5e). Monsters with their own names \
             in the rosters get a file per name, with it on the sheet and the token.",
            if n == 1 { "" } else { "s" },
            self.encounter
        ))
        .size(13)
        .style(text::secondary);

        let list = column(self.plan.actors.iter().map(|a| {
            row![
                text(a.label()).size(13).width(Length::FillPortion(1)),
                text(a.out.clone()).size(11).style(text::secondary).width(Length::FillPortion(1)),
            ]
            .spacing(8)
            .into()
        }))
        .spacing(4);
        let list = container(scrollable(list).height(Length::Shrink))
            .padding(6)
            .max_height(240)
            .width(Length::Fill)
            .style(container::bordered_box);

        let problems = column(
            self.plan.problems.iter().map(|p| text(format!("⚠ {p}")).size(12).style(text::danger).into()),
        )
        .spacing(2);

        let running = matches!(self.state, State::Running);
        let ok = self.can_export().then_some(Msg::Export);
        let folder = column![
            text("Folder").size(14),
            row![
                text_input("A folder to write the files to", &self.folder)
                    .id(FOLDER_INPUT)
                    .on_input_maybe((!running).then_some(Msg::Folder))
                    .on_submit_maybe(ok),
                button("Browse…").on_press_maybe((!running).then_some(Msg::Browse)),
            ]
            .spacing(6),
            text("Made if it doesn't exist. Files with the same names are replaced.")
                .size(12)
                .style(text::secondary),
        ]
        .spacing(4);

        column![intro, list, problems, folder].spacing(10).into()
    }

    fn report<'a>(&'a self, report: &'a Report) -> Element<'a, Msg> {
        let n = report.written.len();
        let mut col = column![text(format!(
            "Wrote {n} file{} to {}",
            if n == 1 { "" } else { "s" },
            self.folder.trim()
        ))
        .size(14)]
        .spacing(6);
        for e in &report.errors {
            col = col.push(text(format!("⚠ {e}")).size(12).style(text::danger));
        }
        if !report.warnings.is_empty() {
            let w = report.warnings.len();
            col = col.push(text(format!("{w} warning{}", if w == 1 { "" } else { "s" })).size(13));
            col = col.push(
                container(
                    scrollable(
                        column(
                            report.warnings.iter().map(|w| text(w.clone()).size(11).style(text::secondary).into()),
                        )
                        .spacing(2),
                    )
                    .height(Length::Shrink),
                )
                .padding(6)
                .max_height(200)
                .width(Length::Fill)
                .style(container::bordered_box),
            );
        }
        col.into()
    }
}
