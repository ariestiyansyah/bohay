//! Inter-process layer over Unix sockets: the JSON control API (`api`) agents
//! drive bohay with, the binary frame `protocol`, and the thin `client` /
//! headless `server` of the render path. See docs/08-ipc-socket-api.md.

pub mod api;
pub mod client;
pub mod protocol;
pub mod server;
