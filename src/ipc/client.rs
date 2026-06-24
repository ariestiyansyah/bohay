//! Thin client (M2): connects to the server, forwards input, and blits the
//! frames it streams back onto the real terminal. Holds no app state.

use std::io::BufReader;
use std::path::Path;
use std::thread;

use anyhow::{anyhow, Result};
use ratatui::crossterm::event::{
    read as read_event, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
    EnableMouseCapture, Event,
};
use ratatui::crossterm::execute;
use ratatui::style::{Color, Style};
use ratatui::widgets::Block;
use ratatui::DefaultTerminal;

use crate::ipc::protocol::{self, ClientMessage, FrameData, ServerMessage};
use crate::ipc::transport::{self, Conn};

fn init_bg_color() -> Color {
    let (r, g, b) = crate::INIT_BG;
    Color::Rgb(r, g, b)
}

pub fn run(sock: &Path) -> Result<()> {
    let stream = transport::connect(sock).map_err(|_| anyhow!("cannot connect to bohay server"))?;
    let mut terminal = ratatui::init();
    // Set the terminal's default background dark BEFORE the first draw, so
    // ratatui's clear-on-resize (and the alt-screen) never flash the terminal's
    // own (often white) background. Then paint a dark frame to cover the gap
    // before the server's first frame.
    crate::set_screen_bg(crate::INIT_BG);
    let _ = terminal.draw(|f| {
        f.render_widget(
            Block::new().style(Style::new().bg(init_bg_color())),
            f.area(),
        );
    });
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    crate::install_tui_panic_hook();
    let result = run_inner(stream, &mut terminal);
    crate::reset_screen_bg();
    let _ = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste
    );
    ratatui::restore();
    result
}

fn run_inner(stream: Conn, terminal: &mut DefaultTerminal) -> Result<()> {
    let truecolor = protocol::truecolor_supported();
    let size = terminal.size()?;
    let mut writer = stream.clone();
    protocol::write_message(
        &mut writer,
        &ClientMessage::Hello {
            version: protocol::PROTOCOL_VERSION,
            cols: size.width,
            rows: size.height,
        },
    )?;

    let mut reader = BufReader::new(stream);
    match protocol::read_message::<_, ServerMessage>(&mut reader)? {
        ServerMessage::Welcome { error: Some(e), .. } => return Err(anyhow!("server: {e}")),
        ServerMessage::Welcome { .. } => {}
        _ => return Err(anyhow!("unexpected handshake")),
    }

    // Input thread: terminal events → the server.
    thread::spawn(move || input_loop(writer));

    // Main thread: blit frames as they arrive.
    let mut screen_bg = crate::INIT_BG;
    loop {
        match protocol::read_message::<_, ServerMessage>(&mut reader) {
            Ok(ServerMessage::Frame(frame)) => {
                // Keep the terminal's clear color in sync with the app's actual
                // background (so a theme change / light theme is honored on the
                // next resize-clear too).
                if let Some(bg) = frame_base_rgb(&frame) {
                    if bg != screen_bg {
                        crate::set_screen_bg(bg);
                        screen_bg = bg;
                    }
                }
                blit(terminal, &frame, truecolor)?;
            }
            Ok(ServerMessage::Notify(msg)) => crate::emit_notification(&msg),
            Ok(ServerMessage::Detach) | Ok(ServerMessage::ServerShutdown { .. }) => break,
            Ok(_) => {}
            Err(_) => break, // server gone
        }
    }
    Ok(())
}

/// The app's background as an RGB triple, from the frame's top-left cell — used
/// to keep the terminal's clear color (OSC 11) in sync with the theme. `None`
/// for non-RGB (e.g. a 256-color downsample), leaving the dark default in place.
fn frame_base_rgb(frame: &FrameData) -> Option<(u8, u8, u8)> {
    match protocol::unpack(frame.cells.first()?.bg) {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

fn input_loop(mut writer: Conn) {
    loop {
        let msg = match read_event() {
            Ok(Event::Key(k)) => ClientMessage::Key(k),
            Ok(Event::Mouse(m)) => ClientMessage::Mouse(m),
            Ok(Event::Resize(w, h)) => ClientMessage::Resize { cols: w, rows: h },
            Ok(Event::Paste(s)) => ClientMessage::Paste(s),
            Ok(_) => continue,
            Err(_) => break,
        };
        if protocol::write_message(&mut writer, &msg).is_err() {
            break;
        }
    }
}

fn blit(terminal: &mut DefaultTerminal, frame: &FrameData, truecolor: bool) -> Result<()> {
    let adjust = |c| if truecolor { c } else { protocol::to_256(c) };
    // The app's background, taken from the top-left cell. Used to fill any area
    // the frame doesn't cover (e.g. the newly-exposed strip mid-resize, before
    // the server sends a frame at the new size) so it never flashes white.
    let fill = frame
        .cells
        .first()
        .map(|c| adjust(protocol::unpack(c.bg)))
        .unwrap_or_else(init_bg_color);
    terminal.draw(|f| {
        let area = f.area();
        f.render_widget(Block::new().style(Style::new().bg(fill)), area);
        let buf = f.buffer_mut();
        for (i, cell) in frame.cells.iter().enumerate() {
            let x = (i as u16) % frame.width;
            let y = (i as u16) / frame.width;
            if x < area.width && y < area.height {
                let target = &mut buf[(x, y)];
                // Guard against control chars in the symbol (ratatui panics on
                // them); the server filters too, but never trust the wire.
                let sym = if cell.symbol.is_empty() || cell.symbol.chars().any(|c| c.is_control()) {
                    " "
                } else {
                    &cell.symbol
                };
                target.set_symbol(sym);
                target.set_fg(adjust(protocol::unpack(cell.fg)));
                target.set_bg(adjust(protocol::unpack(cell.bg)));
                target.modifier = protocol::unpack_mods(cell.mods);
            }
        }
        if let Some((cx, cy)) = frame.cursor {
            if cx < area.width && cy < area.height {
                f.set_cursor_position((cx, cy));
            }
        }
    })?;
    Ok(())
}
