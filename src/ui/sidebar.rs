//! The left sidebar: the NODES (workspaces) and AGENTS lists.

use super::*;

fn attention(s: State) -> u8 {
    match s {
        State::Blocked => 4,
        State::Done => 3,
        State::Working => 2,
        State::Idle => 1,
        State::Unknown => 0,
    }
}

/// Most urgent pane state across a whole workspace.
fn rollup(app: &App, ws_index: usize) -> State {
    let mut best = State::Idle;
    if let Some(ws) = app.workspaces.get(ws_index) {
        for tab in &ws.tabs {
            for id in tab.layout.leaves() {
                let s = pane_state(app, id);
                if attention(s) > attention(best) {
                    best = s;
                }
            }
        }
    }
    best
}

// ── sidebar ───────────────────────────────────────────────────────────────

pub(super) type SidebarHits = (Vec<(usize, Rect)>, Vec<(PaneId, Rect)>, Option<Rect>);

pub(super) fn draw_sidebar(f: &mut Frame, area: Rect, app: &App, t: &Theme) -> SidebarHits {
    let mut ws_rects = Vec::new();
    let mut agent_rects = Vec::new();
    f.render_widget(Block::new().style(Style::new().bg(t.base)), area);
    {
        let buf = f.buffer_mut();
        let x = area.right().saturating_sub(1);
        for y in area.top()..area.bottom() {
            buf[(x, y)]
                .set_symbol("│")
                .set_style(Style::new().fg(t.surface0).bg(t.base));
        }
    }

    let cx = area.x + 2;
    let cw = area.width.saturating_sub(3);
    let line_at = |f: &mut Frame, y: u16, line: Line| {
        if y < area.bottom() {
            f.render_widget(Paragraph::new(line), Rect::new(cx, y, cw, 1));
        }
    };

    // Brand.
    line_at(
        f,
        area.y + 1,
        Line::from(vec![
            Span::styled("❯ ", Style::new().fg(t.accent).bold()),
            Span::styled("bohay", Style::new().fg(t.text).bold()),
            Span::styled("  v0.1", Style::new().fg(t.overlay0)),
        ]),
    );

    // Split the body into two halves: SPACES (top) and AGENTS (bottom),
    // separated by a horizontal divider.
    let body_top = area.y + 3;
    let split = body_top + area.bottom().saturating_sub(body_top) / 2;

    // ── SPACES (top half) ──
    let mut y = body_top;
    line_at(f, y, header("NODES", t));
    // "+" add-workspace button on the right of the SPACES header.
    let new_ws_rect = if area.width >= 8 {
        let rect = Rect::new(area.right().saturating_sub(4), y, 3, 1);
        f.render_widget(
            Paragraph::new(Span::styled(
                " + ",
                Style::new().fg(t.accent).bg(t.sel_bg).bold(),
            )),
            rect,
        );
        Some(rect)
    } else {
        None
    };
    y += 1;
    for (i, ws) in app.workspaces.iter().enumerate() {
        if y + 1 >= split {
            break; // each node needs two rows
        }
        let active = i == app.active_ws;
        // Click target spans both rows of the node.
        ws_rects.push((i, Rect::new(area.x, y, area.width, 2)));
        let st = rollup(app, i);
        let name_style = if active {
            Style::new().fg(t.accent).bold()
        } else {
            Style::new().fg(t.subtext1)
        };
        // Row 1: state dot + node name + git branch.
        let mut line1 = vec![
            Span::raw("  "),
            Span::styled(st.dot(), Style::new().fg(st.color(t))),
            Span::raw(" "),
            Span::styled(ws.name.clone(), name_style),
        ];
        if let Some(b) = &ws.branch {
            line1.push(Span::styled(
                format!("  {b}"),
                Style::new().fg(if active { t.green } else { t.overlay0 }),
            ));
        }
        line_at(f, y, Line::from(line1));
        // Row 2: the project path, shown for every node.
        line_at(
            f,
            y + 1,
            Line::from(Span::styled(
                format!("   {}", short_path(&ws.cwd, cw.saturating_sub(3))),
                Style::new().fg(if active { t.subtext0 } else { t.overlay0 }),
            )),
        );
        if active {
            // One big two-row green selection block.
            let buf = f.buffer_mut();
            for row in [y, y + 1] {
                for x in area.x..area.right().saturating_sub(1) {
                    buf[(x, row)].set_bg(t.sel_bg);
                }
            }
        }
        y += 3; // two content rows + a gap
    }

    // ── divider ──
    {
        let buf = f.buffer_mut();
        for x in (area.x + 1)..area.right().saturating_sub(1) {
            buf[(x, split)]
                .set_symbol("─")
                .set_style(Style::new().fg(t.surface1).bg(t.base));
        }
    }

    // ── AGENTS (bottom half) — only real agent sessions, across all nodes/tabs ──
    let mut y = split + 1;
    line_at(f, y, header("AGENTS", t));
    y += 1;
    let focus = app.layout().focus;
    let mut any = false;
    'agents: for ws in app.workspaces.iter() {
        for (ti, tab) in ws.tabs.iter().enumerate() {
            for id in tab.layout.leaves() {
                let Some(s) = app.status.get(&id) else {
                    continue;
                };
                // Skip plain shells; only show recognised agents (or sessions).
                if !(crate::detect::is_agent(&s.agent) || s.agent_session.is_some()) {
                    continue;
                }
                if y + 1 >= area.bottom() {
                    break 'agents;
                }
                any = true;
                let focused = id == focus;
                let st = pane_state(app, id);
                let name_style = if focused {
                    Style::new().fg(t.accent).bold()
                } else {
                    Style::new().fg(t.subtext1)
                };
                agent_rects.push((id, Rect::new(area.x, y, area.width, 2)));
                // Row 1: state dot + status + agent name.
                line_at(
                    f,
                    y,
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(st.dot(), Style::new().fg(st.color(t))),
                        Span::styled(format!(" {}  ", st.label()), Style::new().fg(st.color(t))),
                        Span::styled(s.agent.clone(), name_style),
                    ]),
                );
                // Row 2: which node + tab this agent runs in.
                line_at(
                    f,
                    y + 1,
                    Line::from(Span::styled(
                        format!("   {} · tab {}", ws.name, ti + 1),
                        Style::new().fg(t.overlay0),
                    )),
                );
                if focused {
                    let buf = f.buffer_mut();
                    for row in [y, y + 1] {
                        for x in area.x..area.right().saturating_sub(1) {
                            buf[(x, row)].set_bg(t.sel_bg);
                        }
                    }
                }
                y += 3; // two content rows + a gap
            }
        }
    }
    if !any {
        line_at(
            f,
            y,
            Line::from(Span::styled(
                "  no active agents",
                Style::new().fg(t.overlay0),
            )),
        );
    }
    (ws_rects, agent_rects, new_ws_rect)
}

fn header(text: &str, t: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::new().fg(t.overlay1).bold(),
    ))
}
