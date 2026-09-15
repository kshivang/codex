use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

fn colors(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> DefaultColors {
    DefaultColors { fg, bg }
}

fn report(slot: u8, rgb: Option<(u8, u8, u8)>) -> EventWithColor {
    let color = rgb.map_or(Color::Reset, |(r, g, b)| Color::Rgb { r, g, b });
    if slot == 10 {
        EventWithColor::ForegroundColor(color)
    } else {
        EventWithColor::BackgroundColor(color)
    }
}

#[test]
fn refresh_preserves_interleaved_input_and_commits_only_a_complete_pair() {
    let mut refresh = PaletteRefresh::default();
    let now = Instant::now();
    let mut output = Vec::new();
    let key = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    let paste = Event::Paste("draft\nsecond line".into());
    let events = [
        EventWithColor::Event(Event::FocusGained),
        report(/*slot*/ 11, Some((245, 247, 251))),
        EventWithColor::Event(key.clone()),
        EventWithColor::Event(paste.clone()),
        report(/*slot*/ 10, Some((20, 26, 36))),
    ];
    let received: Vec<_> = events
        .into_iter()
        .filter_map(|event| refresh.observe(event, now, &mut output))
        .collect();
    assert_eq!(
        received,
        vec![
            PaletteEvent::Input(Event::FocusGained),
            PaletteEvent::Input(key),
            PaletteEvent::Input(paste),
            PaletteEvent::Colors(colors((20, 26, 36), (245, 247, 251))),
        ]
    );
    assert_eq!(output, QUERY);
}

#[test]
fn unanswered_and_malformed_refreshes_can_retry_without_publishing_partial_colors() {
    let mut refresh = PaletteRefresh::default();
    let mut output = Vec::new();
    let start = Instant::now();
    refresh.observe(
        EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
        start,
        &mut output,
    );
    assert_eq!(
        refresh.observe(report(/*slot*/ 10, Some((1, 2, 3))), start, &mut output),
        None
    );
    let later = start + RESPONSE_TIMEOUT;
    assert_eq!(
        refresh.observe(report(/*slot*/ 11, Some((4, 5, 6))), later, &mut output),
        None
    );
    refresh.observe(
        EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
        later,
        &mut output,
    );
    assert_eq!(
        refresh.observe(report(/*slot*/ 10, /*rgb*/ None), later, &mut output),
        None
    );
    refresh.observe(
        EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
        later,
        &mut output,
    );
    refresh.observe(report(/*slot*/ 10, Some((7, 8, 9))), later, &mut output);
    assert_eq!(
        refresh.observe(report(/*slot*/ 11, Some((10, 11, 12))), later, &mut output),
        Some(PaletteEvent::Colors(colors((7, 8, 9), (10, 11, 12))))
    );
    assert_eq!(output, QUERY.repeat(3));
}

#[test]
fn repeated_focus_and_theme_notifications_share_one_pending_request() {
    let mut refresh = PaletteRefresh::default();
    let mut output = Vec::new();
    let now = Instant::now();
    for _ in 0..10 {
        refresh.observe(EventWithColor::Event(Event::FocusGained), now, &mut output);
        refresh.observe(
            EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
            now,
            &mut output,
        );
    }
    assert_eq!(output, QUERY);
}

#[test]
fn unsolicited_replies_do_not_replace_the_palette() {
    let mut refresh = PaletteRefresh::default();
    let mut output = Vec::new();
    for slot in [10, 11] {
        assert_eq!(
            refresh.observe(report(slot, Some((0, 0, 0))), Instant::now(), &mut output),
            None
        );
    }
    assert_eq!(output, Vec::<u8>::new());
}

#[test]
fn failed_output_still_forwards_focus_and_allows_retry() {
    let mut refresh = PaletteRefresh::default();
    let now = Instant::now();
    assert_eq!(
        refresh.observe(
            EventWithColor::Event(Event::FocusGained),
            now,
            &mut &mut [][..]
        ),
        Some(PaletteEvent::Input(Event::FocusGained))
    );
    let mut output = Vec::new();
    refresh.observe(
        EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
        now,
        &mut output,
    );
    assert_eq!(output, QUERY);
}

#[test]
fn composer_style_follows_both_theme_directions_with_draft_intact() {
    let mut refresh = PaletteRefresh::default();
    let mut output = Vec::new();
    let now = Instant::now();
    let mut draft = String::new();
    let mut frames = Vec::new();
    for (index, color) in [
        colors((213, 219, 229), (5, 7, 11)),
        colors((20, 26, 36), (245, 247, 251)),
        colors((213, 219, 229), (5, 7, 11)),
    ]
    .into_iter()
    .enumerate()
    {
        refresh.observe(
            EventWithColor::Event(Event::ColorScheme(crossterm::event::ColorScheme::Light)),
            now,
            &mut output,
        );
        if index == 0
            && let Some(PaletteEvent::Input(Event::Paste(text))) = refresh.observe(
                EventWithColor::Event(Event::Paste("my draft".into())),
                now,
                &mut output,
            )
        {
            draft.push_str(&text);
        }
        refresh.observe(report(/*slot*/ 10, Some(color.fg)), now, &mut output);
        let Some(PaletteEvent::Colors(updated)) =
            refresh.observe(report(/*slot*/ 11, Some(color.bg)), now, &mut output)
        else {
            panic!("missing color pair")
        };
        let area = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 1,
        };
        let mut buffer = Buffer::empty(area);
        crate::terminal_palette::with_test_default_colors(updated, || {
            Paragraph::new(draft.as_str())
                .style(crate::style::user_message_style())
                .render(area, &mut buffer);
        });
        frames.push(buffer);
    }
    assert_eq!(draft, "my draft");
    insta::assert_debug_snapshot!(frames);
}
