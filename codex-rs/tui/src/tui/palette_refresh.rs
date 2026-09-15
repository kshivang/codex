//! Nonblocking color refresh using replies from the sole terminal event reader.
//!
//! A refresh never reads stdin or waits for a reply. Only a complete RGB pair
//! received within the request deadline is published; timeout and malformed
//! replies leave the last good palette intact. Dropping the event source during
//! editor handoff or suspension also drops its pending refresh.

use std::io::Write;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::Event;
use crossterm::event::EventWithColor;
use crossterm::style::Color;

use crate::terminal_probe::DefaultColors;

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(/*millis*/ 250);
const QUERY: &[u8] = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PaletteEvent {
    Input(Event),
    Colors(DefaultColors),
}

struct Pending {
    deadline: Instant,
    fg: Option<(u8, u8, u8)>,
    bg: Option<(u8, u8, u8)>,
}

#[derive(Default)]
pub(super) struct PaletteRefresh {
    pending: Option<Pending>,
}

impl PaletteRefresh {
    pub(super) fn observe(
        &mut self,
        event: EventWithColor,
        now: Instant,
        output: &mut impl Write,
    ) -> Option<PaletteEvent> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now >= pending.deadline)
        {
            self.pending = None;
        }
        let (slot, rgb) = match event {
            EventWithColor::Event(event) => {
                if cfg!(unix)
                    && self.pending.is_none()
                    && matches!(event, Event::FocusGained | Event::ColorScheme(_))
                {
                    match output.write_all(QUERY).and_then(|()| output.flush()) {
                        Ok(()) => {
                            self.pending = Some(Pending {
                                deadline: now + RESPONSE_TIMEOUT,
                                fg: None,
                                bg: None,
                            })
                        }
                        Err(error) => tracing::debug!(%error, "terminal palette query failed"),
                    }
                }
                return (!matches!(event, Event::ColorScheme(_)))
                    .then_some(PaletteEvent::Input(event));
            }
            EventWithColor::ForegroundColor(Color::Rgb { r, g, b }) => (10, (r, g, b)),
            EventWithColor::BackgroundColor(Color::Rgb { r, g, b }) => (11, (r, g, b)),
            EventWithColor::ForegroundColor(_) | EventWithColor::BackgroundColor(_) => {
                self.pending = None;
                return None;
            }
        };
        let pending = self.pending.as_mut()?;
        if slot == 10 {
            pending.fg = Some(rgb);
        } else {
            pending.bg = Some(rgb);
        }
        let (Some(fg), Some(bg)) = (pending.fg, pending.bg) else {
            return None;
        };
        self.pending = None;
        Some(PaletteEvent::Colors(DefaultColors { fg, bg }))
    }
}

#[cfg(all(test, unix))]
#[path = "palette_refresh_tests.rs"]
mod tests;
