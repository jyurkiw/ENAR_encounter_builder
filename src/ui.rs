//! Shared view helpers.

use iced::widget::{center, container, mouse_area, opaque, stack};
use iced::{Color, Element};

/// Draws `content` centred over a dimmed, click-blocking `base`.
pub fn modal<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    stack![
        base.into(),
        opaque(mouse_area(center(opaque(content)).style(|_| container::Style {
            background: Some(Color { a: 0.55, ..Color::BLACK }.into()),
            ..container::Style::default()
        })))
    ]
    .into()
}
