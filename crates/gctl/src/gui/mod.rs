//! The iced configurator window: a pure section state machine (`state`), the
//! device transactions it requests (`io`), and the window over both (`view`).

mod control;
mod dropdown;
pub(crate) mod io;
mod motion;
pub(crate) mod state;
#[cfg(test)]
mod testutil;
mod theme;
mod view;

pub use view::run;
