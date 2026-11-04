use std::fmt::Write;

use crownui::prelude::{AnyView, Signal};
use monitor_engine::system::inspect::ProcessDetails;

use super::folded::folded_text;
use crate::inspector::fields::section;

/// Own processes only, folded until asked: it can hold tokens and passwords.
pub fn view(details: &ProcessDetails, open: Signal<bool>) -> Option<AnyView> {
    let variables = details.environment.as_ref()?;
    let mut body = String::with_capacity(variables.len() * 32);
    for (name, value) in variables {
        let _ = writeln!(body, "{name}={value}");
    }
    Some(section("Environment", folded_text("variables", open, body)))
}
