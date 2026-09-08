// Each `tests/*.rs` file is compiled as its own crate, so any of these
// helpers unused by a particular test binary would otherwise trigger a
// `dead_code` lint there even though other binaries in this workspace use
// it. Allow it at the module boundary rather than per-item.
#[allow(dead_code)]
pub mod cli;
pub mod macros;
#[allow(dead_code)]
pub mod utils;
