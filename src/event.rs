//! Messages flowing into the main loop from input/PTY threads and (in server
//! mode) from client connections.

use std::sync::mpsc::SyncSender;

use ratatui::crossterm::event::{KeyEvent, MouseEvent};

use crate::ids::PaneId;
use crate::ipc::protocol::ServerMessage;

pub enum AppEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    /// The given pane produced output; the screen changed.
    PtyData(PaneId),
    /// The given pane's child process exited.
    PtyExit(PaneId),
    /// A binary client attached (server mode); `frames` receives rendered frames.
    ClientConnected {
        id: u64,
        frames: SyncSender<ServerMessage>,
        cols: u16,
        rows: u16,
    },
    /// A binary client detached.
    ClientDetach {
        id: u64,
    },
    /// A module subprocess finished; fill in its log entry.
    ModuleCommandFinished {
        log_id: u64,
        code: Option<i32>,
        out: String,
        err: String,
    },
}
