//! The bottom status line: prefix hint, key cheatsheet, and right-aligned
//! mode / pane / tab / node readout.

use super::*;

// ── status ──────────────────────────────────────────────────────────────────

pub(super) fn draw_status(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    f.render_widget(Block::new().style(Style::new().bg(t.crust)), area);
    let prefix = app.mode == Mode::Prefix;

    let mut left: Vec<Span> = vec![Span::raw(" ")];
    if prefix {
        left.push(Span::styled(
            " PREFIX ",
            Style::new().fg(t.crust).bg(t.accent).bold(),
        ));
        left.push(Span::raw("  "));
    } else {
        left.push(Span::styled(
            " ⌃Space ",
            Style::new().fg(t.crust).bg(t.accent).bold(),
        ));
        left.push(Span::styled("  prefix   ", Style::new().fg(t.subtext0)));
    }
    left.extend(hint("v", "split →", t));
    left.extend(hint("s", "split ↓", t));
    left.extend(hint("x", "close", t));
    left.extend(hint("c", "tab", t));
    left.extend(hint("hjkl", "move", t));
    left.extend(hint("z", "zoom", t));
    f.render_widget(Paragraph::new(Line::from(left)), area);

    let mode_name = if prefix { "PREFIX" } else { "NORMAL" };
    let mode_color = if prefix { t.accent } else { t.overlay1 };
    let panes = app.layout().len();
    let ws = app.ws();
    let right = Line::from(vec![
        Span::styled(mode_name, Style::new().fg(mode_color).bold()),
        Span::styled("  ·  ", Style::new().fg(t.overlay0)),
        Span::styled(
            format!("{panes} pane{}", if panes == 1 { "" } else { "s" }),
            Style::new().fg(t.subtext0),
        ),
        Span::styled("  ·  ", Style::new().fg(t.overlay0)),
        Span::styled(
            format!("tab {}/{}", ws.active_tab + 1, ws.tabs.len()),
            Style::new().fg(t.subtext0),
        ),
        Span::styled("  ·  ", Style::new().fg(t.overlay0)),
        Span::styled(ws.name.clone(), Style::new().fg(t.subtext1)),
        Span::raw(" "),
    ]);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn hint(key: &str, word: &str, t: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(key.to_string(), Style::new().fg(t.accent).bold()),
        Span::styled(format!(" {word}   "), Style::new().fg(t.subtext0)),
    ]
}
