//! Tabs: panel selection, tab lifecycle, opening files (normal + diff tabs),
//! disk reloads and saving. Split by concern; every child's API is
//! re-exported here so `use tabs::*` in `update/mod.rs` reaches it.

use super::*;

mod lifecycle;
mod open;
mod panel;
mod reload;
mod save;

pub(super) use lifecycle::*;
pub(super) use open::*;
pub(super) use panel::*;
pub(super) use reload::*;
pub(super) use save::*;
