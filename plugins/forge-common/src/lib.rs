//! Code shared by the forge plugins (`forgejo`, `github`): title prefixes, timestamps, the stderr
//! warning log and the blocking `wasi:http` transport. It has no WIT bindings of its own; the
//! plugins map its types to theirs.

pub mod draft;
pub mod http;
pub mod log;
pub mod open_filter;
pub mod timestamp;
