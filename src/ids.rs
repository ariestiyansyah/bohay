//! Identity. `PaneId` is a process-global monotonic counter so a pane keeps its
//! id across splits and moves. (Public base-32 ids land with the data model.)

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PaneId(pub u32);

static NEXT_PANE_ID: AtomicU32 = AtomicU32::new(1);

impl PaneId {
    pub fn alloc() -> Self {
        PaneId(NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed))
    }
}
