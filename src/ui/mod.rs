//! Rendering. compute (resize PTYs) then a pure draw pass: chrome (sidebar,
//! tab bar, status) plus the tiled panes. See docs/06-ui-rendering.md.

pub mod theme;

use std::path::Path;

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::ids::PaneId;
use crate::ui::theme::{State, Theme};

mod borders;
mod panes;
mod sidebar;
mod status;
mod tabbar;

pub fn render(f: &mut Frame, app: &mut App) {
    let t = app.theme.clone();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(t.mantle)), area);

    let [main, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    let (sidebar, content) = if app.sidebar_visible {
        let sw = 30u16.min(main.width.saturating_sub(24));
        if sw >= 18 {
            let [s, c] =
                Layout::horizontal([Constraint::Length(sw), Constraint::Min(0)]).areas(main);
            (Some(s), c)
        } else {
            (None, main)
        }
    } else {
        (None, main)
    };

    let [tabbar, pane_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(content);

    app.last_pane_area = pane_area;

    let focus = app.layout().focus;
    let rects: Vec<(PaneId, Rect)> = if app.zoomed {
        vec![(focus, pane_area)]
    } else {
        app.layout()
            .panes(pane_area)
            .into_iter()
            .map(|p| (p.id, p.rect))
            .collect()
    };
    // Only frame panes when the tab is split; a lone pane needs no border.
    let bordered = rects.len() > 1;
    for (id, rect) in &rects {
        if let Some(content) = pane_content(*rect, bordered) {
            if let Some(p) = app.panes.get_mut(id) {
                p.resize(content.width, content.height);
            }
        }
    }

    let (ws_rects, agent_rects, new_ws_rect) = if let Some(s) = sidebar {
        sidebar::draw_sidebar(f, s, app, &t)
    } else {
        (Vec::new(), Vec::new(), None)
    };
    let (tab_rects, tab_close_rects, tab_prev, tab_next) = tabbar::draw_tabbar(f, tabbar, app, &t);
    // Behind the panes, use the (dark) pane background.
    f.render_widget(Block::new().style(Style::new().bg(t.mantle)), pane_area);
    // The focused pane's ✕ close button, for mouse hit-testing.
    app.pane_close_rect = if bordered {
        rects
            .iter()
            .find(|(id, _)| *id == focus)
            .and_then(|(_, r)| pane_close_rect(*r, bordered))
    } else {
        None
    };
    let cursor = panes::draw_panes(f, &rects, bordered, app, &t);
    // Draw all pane borders in one overlay pass (manual cell-by-cell rendering),
    // then the dot+path+close titles ON each top border row.
    if bordered {
        borders::render_pane_borders(f, &rects, focus, &t);
        panes::draw_pane_titles(f, &rects, focus, app, &t);
    }
    status::draw_status(f, status, app, &t);
    if let Some(p) = cursor {
        f.set_cursor_position(p);
    }
    app.last_cursor = cursor;
    app.pane_rects = rects;
    app.tab_rects = tab_rects;
    app.tab_close_rects = tab_close_rects;
    app.tab_prev_rect = tab_prev;
    app.tab_next_rect = tab_next;
    app.ws_rects = ws_rects;
    app.agent_rects = agent_rects;
    app.new_ws_rect = new_ws_rect;
}

// ── shared layout + state helpers (used across the ui submodules) ──

/// One cell inset on each side, for the border. Used to lay out the box.
fn pane_inner(rect: Rect, bordered: bool) -> Option<Rect> {
    if !bordered {
        if rect.width < 1 || rect.height < 1 {
            return None;
        }
        return Some(rect);
    }
    if rect.width < 4 || rect.height < 4 {
        return None;
    }
    Some(Rect::new(
        rect.x + 1,
        rect.y + 1,
        rect.width - 2,
        rect.height - 2,
    ))
}

/// The terminal content area: inside the box when bordered (the dot+path+close
/// live on the top border row as a title), else just below the header row.
fn pane_content(rect: Rect, bordered: bool) -> Option<Rect> {
    if bordered {
        return pane_inner(rect, true);
    }
    let c = Rect::new(
        rect.x,
        rect.y + 1,
        rect.width,
        rect.height.saturating_sub(1),
    );
    if c.width < 1 || c.height < 1 {
        return None;
    }
    Some(c)
}

/// Rect of the ✕ close button at the right of a pane's top-border title row.
fn pane_close_rect(area: Rect, bordered: bool) -> Option<Rect> {
    if !bordered || area.width < 9 {
        return None;
    }
    Some(Rect::new(area.x + area.width - 4, area.y, 3, 1))
}

fn pane_state(app: &App, id: PaneId) -> State {
    app.status
        .get(&id)
        .map(|s| s.state)
        .unwrap_or(State::Unknown)
}

/// Collapse `$HOME` to `~` and truncate from the left to fit `max` columns.
fn short_path(p: &Path, max: u16) -> String {
    let mut s = p.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = s.strip_prefix(&home) {
            s = format!("~{rest}");
        }
    }
    let max = max as usize;
    if s.chars().count() > max && max > 1 {
        let tail: String = s
            .chars()
            .rev()
            .take(max - 1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    } else {
        s
    }
}
