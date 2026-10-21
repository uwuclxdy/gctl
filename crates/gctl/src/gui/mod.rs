//! The iced configurator window: a pure section state machine (`state`), the
//! device transactions it requests (`io`), and the window over both (`view`).

pub(crate) mod io;
pub(crate) mod state;
mod view;

pub use view::run;
