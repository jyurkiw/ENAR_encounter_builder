//! The encounter description's own window: the compiled pages, and a button
//! to save the PDF.

use iced::widget::{button, column, container, image, row, rule, scrollable, space, text};
use iced::{Alignment, Element, Length, window};
use std::path::PathBuf;

pub struct DescriptionWindow {
    pub id: window::Id,
    /// The encounter's title, for the window's title and the PDF's name.
    pub title: String,
    /// Where Save PDF starts: the encounter file's folder.
    pub start_dir: PathBuf,
    pub state: State,
    /// What the last Save PDF did: the path written, or why it failed.
    pub saved: Option<Result<PathBuf, String>>,
}

pub enum State {
    Compiling,
    Ready { pdf: PathBuf, pages: Vec<image::Handle>, warnings: Vec<String> },
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Msg {
    SavePdf,
    /// Where to save it, or None when the dialog was cancelled.
    SaveTo(Option<PathBuf>),
}

impl DescriptionWindow {
    pub fn pdf(&self) -> Option<&PathBuf> {
        match &self.state {
            State::Ready { pdf, .. } => Some(pdf),
            _ => None,
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let note: Element<'_, Msg> = match &self.saved {
            Some(Ok(path)) => text(format!("Saved {}", path.display())).size(12).style(text::success).into(),
            Some(Err(e)) => text(e.clone()).size(12).style(text::danger).into(),
            None => space().into(),
        };
        let toolbar = container(
            row![
                text(self.title.clone()).size(16),
                space::horizontal(),
                note,
                button(text("Save PDF…").size(14))
                    .padding([6, 12])
                    .on_press_maybe(self.pdf().is_some().then_some(Msg::SavePdf)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .width(Length::Fill)
        .style(container::rounded_box);

        let body: Element<'_, Msg> = match &self.state {
            State::Compiling => container(text("Compiling the description…").size(14))
                .center(Length::Fill)
                .into(),
            State::Failed(e) => container(
                column![text("The description couldn't be compiled").size(16), text(e.clone()).size(12).style(text::danger)]
                    .spacing(8)
                    .max_width(640),
            )
            .center(Length::Fill)
            .into(),
            State::Ready { pages, warnings, .. } => {
                let mut col = column![].spacing(12).padding(12).align_x(Alignment::Center);
                if !warnings.is_empty() {
                    col = col.push(
                        column(warnings.iter().map(|w| text(format!("⚠ {w}")).size(12).style(text::secondary).into()))
                            .spacing(2)
                            .width(Length::Fill),
                    );
                }
                for page in pages {
                    col = col.push(
                        container(image(page.clone()).width(Length::Fill))
                            .max_width(900)
                            .style(container::bordered_box),
                    );
                }
                scrollable(col).height(Length::Fill).into()
            }
        };
        column![toolbar, rule::horizontal(1), body].into()
    }
}
