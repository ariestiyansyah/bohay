//! Color palette. "bohay vr46" — a near-black dark UI with a fluorescent
//! stabilo/Valentino-Rossi green accent for active/selected elements.

use ratatui::style::Color;

#[derive(Clone)]
pub struct Theme {
    // surfaces (dark → light)
    pub crust: Color,
    pub mantle: Color,
    pub base: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub overlay0: Color,
    pub overlay1: Color,
    // text
    pub subtext0: Color,
    pub subtext1: Color,
    pub text: Color,
    // accents
    pub accent: Color,       // fluo green — brand, focus, active/selected
    pub sel_bg: Color,       // selection background (dark green tint)
    pub border: Color,       // pane border (unfocused) — grey
    pub border_focus: Color, // pane border (focused) — light grey/white
    pub green: Color,
    pub mint: Color,
    pub amber: Color,
    pub coral: Color,
}

impl Theme {
    pub fn noir() -> Self {
        let rgb = |r, g, b| Color::Rgb(r, g, b);
        Theme {
            crust: rgb(0x07, 0x07, 0x09),
            mantle: rgb(0x11, 0x11, 0x16), // pane background + inactive tabs + tab bar
            base: rgb(0x20, 0x20, 0x28),   // sidebar — a bit lighter than the pane
            surface0: rgb(0x1a, 0x1a, 0x20),
            surface1: rgb(0x25, 0x25, 0x2d),
            overlay0: rgb(0x4a, 0x4a, 0x54),
            overlay1: rgb(0x68, 0x68, 0x73),
            subtext0: rgb(0x93, 0x93, 0x9f),
            subtext1: rgb(0xb6, 0xb6, 0xc0),
            text: rgb(0xe7, 0xe7, 0xed),
            accent: rgb(0xc6, 0xff, 0x1a), // stabilo / VR46 fluo green
            sel_bg: rgb(0x33, 0x45, 0x0e), // dark green selection — stands out from the sidebar
            border: rgb(0x52, 0x52, 0x5c), // muted grey (unfocused) — heavy glyphs stay solid
            border_focus: rgb(0x8c, 0x8c, 0x96), // medium grey (focused) — not white
            green: rgb(0x8f, 0xbc, 0x7a),  // sage (idle)
            mint: rgb(0x6f, 0xc6, 0xa3),   // mint (done)
            amber: rgb(0xe0, 0x9a, 0x4d),  // amber-orange (working)
            coral: rgb(0xe0, 0x6c, 0x66),  // coral (blocked)
        }
    }

    /// Downsample every color to the nearest xterm-256 index, for terminals
    /// without truecolor support.
    pub fn to_256(&self) -> Theme {
        let c = crate::ipc::protocol::to_256;
        Theme {
            crust: c(self.crust),
            mantle: c(self.mantle),
            base: c(self.base),
            surface0: c(self.surface0),
            surface1: c(self.surface1),
            overlay0: c(self.overlay0),
            overlay1: c(self.overlay1),
            subtext0: c(self.subtext0),
            subtext1: c(self.subtext1),
            text: c(self.text),
            accent: c(self.accent),
            sel_bg: c(self.sel_bg),
            border: c(self.border),
            border_focus: c(self.border_focus),
            green: c(self.green),
            mint: c(self.mint),
            amber: c(self.amber),
            coral: c(self.coral),
        }
    }
}

/// Agent / pane activity state. Drives sidebar glyphs and colors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Blocked,
    Working,
    Done,
    Idle,
    Unknown,
}

impl State {
    pub fn dot(self) -> &'static str {
        match self {
            State::Idle | State::Unknown => "○",
            _ => "●",
        }
    }

    pub fn color(self, t: &Theme) -> Color {
        match self {
            State::Blocked => t.coral,
            State::Working => t.amber,
            State::Done => t.mint,
            State::Idle => t.green,
            State::Unknown => t.overlay0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            State::Blocked => "blocked",
            State::Working => "working",
            State::Done => "done",
            State::Idle => "idle",
            State::Unknown => "—",
        }
    }
}
