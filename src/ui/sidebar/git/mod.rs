//! Git panel: change tree, commit box, fetch/pull/push actions.
//!
//! `layout` computes the panel geometry once (rows, fixed top block, button
//! columns); `render` draws it and `hit` maps mouse clicks through the very same
//! layout, so visuals and click targets never drift apart.

mod hit;
mod layout;
mod render;

pub use hit::{GitHit, git_hit};
pub(super) use render::render;
