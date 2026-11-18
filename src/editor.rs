//! The encounter editor: the main area for an open encounter. It does what
//! the old XP spreadsheet did, for every roster of the encounter: each
//! monster's CR and XP (read from its monster file), the roster's total, and
//! what's left against the 2024 DMG's Low, Moderate and High budgets for the
//! roster's party.

use super::*;
use crate::encounter::{Reference, RosterRef, RosterSel};
use crate::monsters::MonsterEntry;
use crate::xp::{self, Difficulty, Fit, Tier};
use iced::widget::{button, column, container, row, rule, scrollable, space, text, text_input};
use iced::{Border, Color, Theme};

/// An edit to the open encounter.
#[derive(Debug, Clone)]
pub enum Edit {
    Id(String),
    Level(u32),
    Pcs(u32),
    /// The easiest difficulty a group aims for (raises the top if needed).
    MinDifficulty(usize, Tier),
    /// The hardest difficulty a group aims for (lowers the bottom if needed).
    MaxDifficulty(usize, Tier),
    GroupName(usize, String),
    AddGroup,
    RemoveGroup(usize),
    AddAlt(usize),
    RemoveAlt(usize, usize),
    AltLevel(usize, usize, u32),
    AltPcs(usize, usize, u32),
    /// Where "Add to roster" puts monsters: set by clicking a roster.
    Target(RosterRef),
    Count(RosterRef, usize, u32),
    RemoveRow(RosterRef, usize),
    DisplayName(usize, String),
    ShortName(usize, String),
    RemoveReference(usize),
}

/// One roster row, worked out for display.
pub struct Row<'a> {
    pub id: &'a str,
    pub name: String,
    pub cr: Option<&'a str>,
    pub count: u32,
    /// XP for all of them; None when the monster or its CR isn't known.
    pub xp: Option<u32>,
    pub found: bool,
}

impl Document {
    /// Applies an edit, marking the document changed if it did anything.
    pub fn apply(&mut self, edit: Edit) {
        // Working in a roster picks it for "Add to roster".
        let touched = match &edit {
            Edit::Count(at, ..) | Edit::RemoveRow(at, _) => Some(*at),
            Edit::AltLevel(g, a, _) | Edit::AltPcs(g, a, _) => Some(RosterRef { group: *g, roster: RosterSel::Alt(*a) }),
            _ => None,
        };
        if let Some(at) = touched
            && self.encounter.roster(at).is_some()
        {
            self.target = at;
        }
        let e = &mut self.encounter;
        let changed = match edit {
            Edit::Id(v) => set(&mut e.id, v),
            Edit::Level(v) => set(&mut e.level, v.clamp(1, 20)),
            Edit::Pcs(v) => set(&mut e.num_pcs, v.max(1)),
            Edit::MinDifficulty(g, t) => e.groups.get_mut(g).is_some_and(|g| {
                let top = g.max_difficulty.max(t);
                let raised = set(&mut g.max_difficulty, top);
                set(&mut g.min_difficulty, t) | raised
            }),
            Edit::MaxDifficulty(g, t) => e.groups.get_mut(g).is_some_and(|g| {
                let bottom = g.min_difficulty.min(t);
                let lowered = set(&mut g.min_difficulty, bottom);
                set(&mut g.max_difficulty, t) | lowered
            }),
            Edit::GroupName(g, v) => e.groups.get_mut(g).is_some_and(|g| set(&mut g.name, v)),
            Edit::AddGroup => {
                self.target = e.add_group();
                true
            }
            Edit::RemoveGroup(g) => {
                if g < e.groups.len() && e.groups.len() > 1 {
                    e.groups.remove(g);
                    self.target = match self.target {
                        t if t.group == g => RosterRef::FIRST,
                        t if t.group > g => RosterRef { group: t.group - 1, ..t },
                        t => t,
                    };
                    true
                } else {
                    false
                }
            }
            Edit::AddAlt(g) => match e.add_alt_roster(g) {
                Some(at) => {
                    self.target = at;
                    true
                }
                None => false,
            },
            Edit::RemoveAlt(g, a) => match e.groups.get_mut(g) {
                Some(group) if a < group.alt_rosters.len() => {
                    group.alt_rosters.remove(a);
                    if self.target.group == g {
                        self.target.roster = match self.target.roster {
                            RosterSel::Alt(i) if i == a => RosterSel::Base,
                            RosterSel::Alt(i) if i > a => RosterSel::Alt(i - 1),
                            r => r,
                        };
                    }
                    true
                }
                _ => false,
            },
            Edit::AltLevel(g, a, v) => alt(e, g, a).is_some_and(|r| set(&mut r.level, v.clamp(1, 20))),
            Edit::AltPcs(g, a, v) => alt(e, g, a).is_some_and(|r| set(&mut r.num_pcs, v.max(1))),
            Edit::Target(at) => {
                if e.roster(at).is_some() {
                    self.target = at;
                }
                false // where to add isn't part of the file
            }
            Edit::Count(at, row, v) => e
                .roster_mut(at)
                .and_then(|r| r.get_mut(row))
                .is_some_and(|m| set(&mut m.count, v.max(1))),
            Edit::RemoveRow(at, row) => match e.roster_mut(at) {
                Some(r) if row < r.len() => {
                    r.remove(row);
                    true
                }
                _ => false,
            },
            Edit::DisplayName(i, v) => e.references.get_mut(i).is_some_and(|r| set(&mut r.display_name, non_empty(v))),
            Edit::ShortName(i, v) => e.references.get_mut(i).is_some_and(|r| set(&mut r.short_name, non_empty(v))),
            Edit::RemoveReference(i) => {
                let unused = e.references.get(i).is_some_and(|r| e.unused_references().contains(&r.id.as_str()));
                if unused {
                    e.references.remove(i);
                }
                unused
            }
        };
        if changed {
            self.dirty = true;
        }
    }
}

fn set<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

fn non_empty(v: String) -> Option<String> {
    (!v.is_empty()).then_some(v)
}

fn alt(e: &mut Encounter, g: usize, a: usize) -> Option<&mut encounter::AltRoster> {
    e.groups.get_mut(g)?.alt_rosters.get_mut(a)
}

/// A path for comparing: forward slashes, lowercase (Windows paths).
fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

impl App {
    /// The scanned file and monster a reference points at, if it's there.
    pub fn referenced_monster(&self, r: &Reference) -> Option<(&monsters::MonsterFile, &MonsterEntry)> {
        let path = self.config.resolve(&r.file);
        self.sources
            .iter()
            .flat_map(|s| &s.files)
            .filter(|f| same_path(&f.path, &path))
            .find_map(|f| f.monsters.iter().find(|m| m.id == r.monster).map(|m| (f, m)))
    }

    /// A roster's rows: names, CRs and XP from the monster files.
    pub fn rows<'a>(&'a self, e: &'a Encounter, at: RosterRef) -> Vec<Row<'a>> {
        e.roster(at)
            .into_iter()
            .flatten()
            .map(|entry| {
                let reference = e.references.iter().find(|r| r.id == entry.id);
                let monster = reference.and_then(|r| self.referenced_monster(r)).map(|(_, m)| m);
                let name = reference
                    .and_then(|r| r.display_name.clone())
                    .or_else(|| monster.map(|m| m.name.clone()))
                    .unwrap_or_else(|| entry.id.clone());
                let cr = monster.and_then(|m| m.cr.as_deref());
                Row {
                    id: &entry.id,
                    name,
                    cr,
                    count: entry.count,
                    xp: cr.and_then(xp::for_cr).map(|x| x * entry.count),
                    found: monster.is_some(),
                }
            })
            .collect()
    }

    /// The open encounter, for editing.
    pub fn encounter_editor<'a>(&'a self, doc: &'a Document) -> Element<'a, Message> {
        let e = &doc.encounter;
        let id = doc.id;
        let edit = move |m: Edit| Message::Edit(id, m);

        let header = column![
            row![
                labelled("Encounter id", text_input("bridge-ambush", &e.id).on_input(move |v| edit(Edit::Id(v))).width(240)),
                labelled("Level", stepper(e.level, 1, 20, move |v| edit(Edit::Level(v)))),
                labelled("PCs", stepper(e.num_pcs, 1, 99, move |v| edit(Edit::Pcs(v)))),
            ]
            .spacing(16)
            .align_y(Alignment::End)
            .wrap(),
            text(match &doc.path {
                Some(p) => p.display().to_string(),
                None => "Not saved yet".into(),
            })
            .size(12)
            .style(text::secondary),
        ]
        .spacing(4);

        let mut body = column![header].spacing(14);
        for (g, group) in e.groups.iter().enumerate() {
            body = body.push(self.group_card(doc, g, group));
        }
        body = body.push(button(text("+ Group").size(13)).style(button::secondary).on_press(edit(Edit::AddGroup)));
        body = body.push(self.references_card(doc));

        let problems = e.problems();
        if !problems.is_empty() {
            body = body.push(
                column(problems.into_iter().map(|p| text(format!("⚠ {p}")).size(12).style(text::warning).into()))
                    .spacing(2),
            );
        }
        scrollable(body.padding(iced::Padding::ZERO.right(12))).height(Length::Fill).into()
    }

    fn group_card<'a>(&'a self, doc: &'a Document, g: usize, group: &'a encounter::Group) -> Element<'a, Message> {
        let id = doc.id;
        let edit = move |m: Edit| Message::Edit(id, m);
        let mut col = column![
            row![
                text_input("Group name", &group.name).on_input(move |v| edit(Edit::GroupName(g, v))).size(16),
                button(text("Remove group").size(12))
                    .style(button::secondary)
                    .on_press_maybe((doc.encounter.groups.len() > 1).then(|| edit(Edit::RemoveGroup(g)))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            // Each group aims for its own difficulty.
            row![
                text("Intended difficulty").size(12).style(text::secondary),
                segmented(group.min_difficulty, move |t| edit(Edit::MinDifficulty(g, t))),
                text("to").size(13),
                segmented(group.max_difficulty, move |t| edit(Edit::MaxDifficulty(g, t))),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .wrap(),
        ]
        .spacing(10);

        col = col.push(self.roster_card(doc, RosterRef { group: g, roster: RosterSel::Base }));
        for a in 0..group.alt_rosters.len() {
            col = col.push(self.roster_card(doc, RosterRef { group: g, roster: RosterSel::Alt(a) }));
        }
        col = col.push(
            button(text("+ Alternate roster").size(13)).style(button::secondary).on_press(edit(Edit::AddAlt(g))),
        );
        container(col).padding(12).width(Length::Fill).style(container::bordered_box).into()
    }

    fn roster_card<'a>(&'a self, doc: &'a Document, at: RosterRef) -> Element<'a, Message> {
        let id = doc.id;
        let edit = move |m: Edit| Message::Edit(id, m);
        let e = &doc.encounter;
        let (level, pcs) = e.party(at).unwrap_or((1, 1));
        let targeted = doc.target == at;

        let mut title = row![].spacing(12).align_y(Alignment::Center);
        title = match at.roster {
            RosterSel::Base => title.push(text(format!("Base roster · level {level} · {pcs} PCs")).size(15)),
            RosterSel::Alt(a) => title
                .push(text("Alternate roster").size(15))
                .push(labelled_inline("Level", stepper(level, 1, 20, move |v| edit(Edit::AltLevel(at.group, a, v)))))
                .push(labelled_inline("PCs", stepper(pcs, 1, 99, move |v| edit(Edit::AltPcs(at.group, a, v)))))
                .push(button(text("Remove").size(12)).style(button::secondary).on_press(edit(Edit::RemoveAlt(at.group, a)))),
        };
        if targeted {
            title = title.push(space::horizontal()).push(text("Adding here").size(12).style(text::success));
        }

        let rows = self.rows(e, at);
        let mut table = column![row![
            text("Monster").size(12).style(text::secondary).width(Length::Fill),
            text("CR").size(12).style(text::secondary).width(CR_W),
            text("Count").size(12).style(text::secondary).width(COUNT_W),
            text("XP").size(12).style(text::secondary).width(XP_W),
            space().width(REMOVE_W),
        ]]
        .spacing(2);
        if rows.is_empty() {
            table = table.push(
                text("No monsters. Pick one in the browser and use \"Add to roster\" in its stat block.")
                    .size(12)
                    .style(text::secondary),
            );
        }
        for (i, r) in rows.iter().enumerate() {
            let mut name = column![
                button(text(r.name.clone()).size(14))
                    .padding([2, 0])
                    .style(button::text)
                    .on_press(Message::RosterMonster(id, at, r.id.to_string()))
            ];
            if !r.found {
                name = name.push(text(format!("{} isn't in the monster folders", r.id)).size(11).style(text::danger));
            }
            table = table.push(
                row![
                    name.width(Length::Fill),
                    text(r.cr.unwrap_or("?")).width(CR_W),
                    container(stepper(r.count, 1, 999, move |v| edit(Edit::Count(at, i, v)))).width(COUNT_W),
                    text(r.xp.map(|x| xp::thousands(x.into())).unwrap_or_else(|| "?".into())).width(XP_W),
                    button(text("×").size(13))
                        .padding([2, 6])
                        .width(REMOVE_W)
                        .style(button::text)
                        .on_press(edit(Edit::RemoveRow(at, i))),
                ]
                .align_y(Alignment::Center),
            );
        }

        let total: u32 = rows.iter().filter_map(|r| r.xp).sum();
        let count: u32 = rows.iter().map(|r| r.count).sum();
        let unknown = rows.iter().any(|r| r.xp.is_none());
        // What the spreadsheet did: the total against the target range for
        // this roster's party, worked out from the intended difficulty.
        let mut totals = row![
            text(format!("Total {} XP", xp::thousands(total.into()))).size(14),
            text(format!("{count} monsters")).size(13).style(text::secondary),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        let mut summary = column![].spacing(4);
        if let Some(budget) = xp::budget(level, pcs) {
            let difficulty = Difficulty::of(total, budget);
            totals = totals.push(text(format!("Rated {}", difficulty.label())).size(13).style(match difficulty {
                Difficulty::OverBudget => text::danger,
                _ => text::secondary,
            }));
            let group = &e.groups[at.group];
            let (from, to) = (group.min_difficulty, group.max_difficulty);
            let range = budget.target(from, to);
            let aim = if from == to { from.to_string() } else { format!("{from} to {to}") };
            let (fit, style): (String, fn(&Theme) -> text::Style) = match Fit::of(total, range) {
                Fit::Short { need, room } => (
                    format!("Add {} – {} XP", xp::thousands(need.into()), xp::thousands(room.into())),
                    text::warning,
                ),
                Fit::InRange(n) => (format!("In range · {} XP to spare", xp::thousands(n.into())), text::success),
                Fit::Over(n) => (format!("{} XP over", xp::thousands(n.into())), text::danger),
            };
            summary = summary
                .push(totals)
                .push(
                    row![
                        text(format!(
                            "Target ({aim}): {} – {} XP",
                            xp::thousands(range.0.into()),
                            xp::thousands(range.1.into())
                        ))
                        .size(14),
                        text(fit).size(14).style(style),
                    ]
                    .spacing(16)
                    .wrap(),
                )
                .push(
                    text(format!(
                        "Budgets for level {level}, {pcs} PCs: Low {} · Moderate {} · High {}",
                        xp::thousands(budget.low.into()),
                        xp::thousands(budget.moderate.into()),
                        xp::thousands(budget.high.into())
                    ))
                    .size(12)
                    .style(text::secondary),
                );
        } else {
            summary = summary.push(totals);
        }
        if unknown {
            summary = summary.push(text("Some monsters' XP is unknown, so the total is low.").size(12).style(text::warning));
        }

        let dropping = self.drag.as_ref().is_some_and(|d| d.active) && self.drop_hover == Some(at);
        let card = container(column![title, table, rule::horizontal(1), summary].spacing(8))
            .padding(10)
            .width(Length::Fill)
            .style(move |theme: &Theme| roster_style(theme, targeted, dropping));
        // A drop target for monsters dragged from the browser.
        // Clicking the roster picks it for "Add to roster" (clicks on its
        // buttons pick it too, through their edits), and it's a drop target
        // for monsters dragged from the browser.
        iced::widget::mouse_area(card)
            .on_press(edit(Edit::Target(at)))
            .on_enter(Message::DropHover(at))
            .on_exit(Message::DropLeave(at))
            .into()
    }

    fn references_card<'a>(&'a self, doc: &'a Document) -> Element<'a, Message> {
        let id = doc.id;
        let edit = move |m: Edit| Message::Edit(id, m);
        let e = &doc.encounter;
        let unused = e.unused_references();
        let mut col = column![
            text("Monster references").size(15),
            text("The names the encounter prints for its monsters. Blank names use the stat block's.")
                .size(12)
                .style(text::secondary),
        ]
        .spacing(6);
        if e.references.is_empty() {
            col = col.push(text("None yet.").size(12).style(text::secondary));
        }
        for (i, r) in e.references.iter().enumerate() {
            let monster = self.referenced_monster(r).map(|(_, m)| m);
            let name = monster.map(|m| m.label()).unwrap_or_else(|| format!("{} (not found)", r.monster));
            // Two lines, so the name fields keep a usable width in a
            // narrow main area (both side panels open).
            let mut heading = row![
                column![text(name).size(14), text(r.file.as_str()).size(11).style(text::secondary)].width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if unused.contains(&r.id.as_str()) {
                heading = heading.push(text("unused").size(12).style(text::warning)).push(
                    button(text("Remove").size(12)).style(button::secondary).on_press(edit(Edit::RemoveReference(i))),
                );
            }
            let names = row![
                text_input("Display name", r.display_name.as_deref().unwrap_or_default())
                    .on_input(move |v| edit(Edit::DisplayName(i, v)))
                    .width(Length::FillPortion(3)),
                text_input("Short name", r.short_name.as_deref().unwrap_or_default())
                    .on_input(move |v| edit(Edit::ShortName(i, v)))
                    .width(Length::FillPortion(2)),
            ]
            .spacing(8);
            col = col.push(column![heading, names].spacing(4));
        }
        container(col).padding(12).width(Length::Fill).style(container::bordered_box).into()
    }
}

// Roster table column widths; the monster name takes the rest.
const CR_W: f32 = 40.0;
const COUNT_W: f32 = 96.0;
const XP_W: f32 = 64.0;
const REMOVE_W: f32 = 28.0;

/// − value + : a small whole-number control.
fn stepper<'a>(value: u32, min: u32, max: u32, on: impl Fn(u32) -> Message + 'a) -> Element<'a, Message> {
    row![
        button(text("−").size(13)).padding([2, 7]).style(button::secondary).on_press_maybe((value > min).then(|| on(value - 1))),
        text(value.to_string()).width(30).align_x(Alignment::Center),
        button(text("+").size(13)).padding([2, 7]).style(button::secondary).on_press_maybe((value < max).then(|| on(value + 1))),
    ]
    .spacing(2)
    .align_y(Alignment::Center)
    .into()
}

/// Low | Moderate | High, with the chosen one highlighted.
fn segmented<'a>(selected: Tier, on: impl Fn(Tier) -> Message + 'a) -> Element<'a, Message> {
    row(Tier::ALL.into_iter().map(|t| {
        button(text(t.to_string()).size(12))
            .padding([3, 8])
            .style(if t == selected { button::primary } else { button::secondary })
            .on_press(on(t))
            .into()
    }))
    .spacing(1)
    .into()
}

fn labelled<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(12).style(text::secondary), control.into()].spacing(2).into()
}

fn labelled_inline<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(12).style(text::secondary), control.into()].spacing(4).align_y(Alignment::Center).into()
}

/// A roster's box, outlined in the accent colour when monsters are added to
/// it, and tinted while a dragged monster is over it.
fn roster_style(theme: &Theme, targeted: bool, dropping: bool) -> container::Style {
    let p = theme.extended_palette();
    let background = if dropping { Color { a: 0.25, ..p.primary.weak.color } } else { p.background.weakest.color };
    container::Style {
        background: Some(background.into()),
        border: Border {
            color: if targeted || dropping { p.primary.base.color } else { Color { a: 0.4, ..p.background.strong.color } },
            width: if targeted || dropping { 2.0 } else { 1.0 },
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}
