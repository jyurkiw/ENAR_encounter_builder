//! The Encounter Description modal: which rosters go into the description,
//! and which stat blocks go into its bestiary.

use crate::description::{BlockOption, RosterOption};
use iced::widget::{button, checkbox, column, container, row, rule, scrollable, space, text};
use iced::{Alignment, Element, Length};

pub struct DescriptionDialog {
    /// The encounter's document, so the compile uses the encounter it was opened on.
    pub doc: u64,
    pub encounter: String,
    pub rosters: Vec<(RosterOption, bool)>,
    /// The stat blocks of the chosen rosters, each with whether it's chosen.
    pub blocks: Vec<(BlockOption, bool)>,
    /// Roster monsters with no reference, so no stat block.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Roster(usize, bool),
    Block(usize, bool),
    AllBlocks(bool),
    /// Okay: compile the description.
    Compile,
    Close,
}

impl DescriptionDialog {
    pub fn chosen_rosters(&self) -> Vec<crate::encounter::RosterRef> {
        self.rosters.iter().filter(|(_, on)| *on).map(|(r, _)| r.at).collect()
    }

    pub fn chosen_blocks(&self) -> Vec<BlockOption> {
        self.blocks.iter().filter(|(_, on)| *on).map(|(b, _)| b.clone()).collect()
    }

    /// Sets the stat blocks for the chosen rosters: ones already listed keep
    /// their tick, new ones start ticked.
    pub fn set_blocks(&mut self, blocks: Vec<BlockOption>, missing: Vec<String>) {
        let old = std::mem::take(&mut self.blocks);
        self.blocks = blocks
            .into_iter()
            .map(|b| {
                let on = old.iter().find(|(o, _)| *o == b).is_none_or(|(_, on)| *on);
                (b, on)
            })
            .collect();
        self.missing = missing;
    }

    pub fn can_compile(&self) -> bool {
        self.rosters.iter().any(|(_, on)| *on)
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let rosters = column(self.rosters.iter().enumerate().map(|(i, (r, on))| {
            checkbox(*on).label(r.label.clone()).on_toggle(move |v| Msg::Roster(i, v)).text_size(13).into()
        }))
        .spacing(6);

        let all_on = self.blocks.iter().all(|(_, on)| *on);
        let blocks: Element<'_, Msg> = if self.blocks.is_empty() {
            text("No stat blocks: choose a roster with monsters.").size(13).style(text::secondary).into()
        } else {
            container(
                scrollable(
                    column(self.blocks.iter().enumerate().map(|(i, (b, on))| {
                        checkbox(*on).label(b.label()).on_toggle(move |v| Msg::Block(i, v)).text_size(13).into()
                    }))
                    .spacing(6),
                )
                .height(Length::Shrink),
            )
            .padding(6)
            .max_height(240)
            .width(Length::Fill)
            .style(container::bordered_box)
            .into()
        };
        let missing = column(self.missing.iter().map(|m| {
            text(format!("⚠ {m} has no reference, so it has no stat block.")).size(12).style(text::danger).into()
        }))
        .spacing(2);

        let body = column![
            text(format!(
                "A summary of each chosen roster (name, number, CR, AC, HP and XP) for {}, then a bestiary of \
                 the chosen stat blocks with the names they're used under on top.",
                self.encounter
            ))
            .size(13)
            .style(text::secondary),
            text("Rosters").size(14),
            rosters,
            row![
                text("Bestiary").size(14),
                space::horizontal(),
                button(text(if all_on { "None" } else { "All" }).size(12))
                    .style(button::text)
                    .padding([2, 6])
                    .on_press_maybe((!self.blocks.is_empty()).then_some(Msg::AllBlocks(!all_on))),
            ]
            .align_y(Alignment::Center),
            blocks,
            missing,
        ]
        .spacing(10);

        let footer = row![
            space::horizontal(),
            button("Cancel").style(button::secondary).on_press(Msg::Close),
            button("Okay").on_press_maybe(self.can_compile().then_some(Msg::Compile)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        container(column![text("Encounter Description").size(20), body, rule::horizontal(1), footer].spacing(14))
            .padding(20)
            .width(600)
            .style(container::rounded_box)
            .into()
    }
}
