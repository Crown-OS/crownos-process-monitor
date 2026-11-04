use crownui::ext::{Key, Keybind, Modifiers};

/// Ctrl plus `key`, e.g. `ctrl('f')` for Ctrl+F.
pub const fn ctrl(key: char) -> Keybind {
    Keybind {
        key: Key::Character(key),
        modifiers: Modifiers {
            ctrl_key: true,
            ..Modifiers::NONE
        },
    }
}

/// Ctrl+1 for the first of up to nine items, Ctrl+2 for the second, and so on.
pub fn ctrl_digit(position: usize) -> Option<Keybind> {
    let digit = u32::try_from(position + 1).ok()?;
    char::from_digit(digit, 10).map(ctrl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_follow_positions() {
        assert_eq!(ctrl_digit(0), Some(ctrl('1')));
        assert_eq!(ctrl_digit(5), Some(ctrl('6')));
        assert_eq!(ctrl_digit(9), None);
    }
}
