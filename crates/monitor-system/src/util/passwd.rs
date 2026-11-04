use std::collections::HashMap;
use std::sync::Arc;

const PASSWD: &str = "/etc/passwd";

/// `(uid, name)` pairs of a `/etc/passwd` text.
pub(crate) fn passwd_entries(text: &str) -> impl Iterator<Item = (u32, &str)> {
    text.lines().filter_map(|line| {
        let mut columns = line.split(':');
        let name = columns.next()?;
        let uid = columns.nth(1)?.parse().ok()?;
        Some((uid, name))
    })
}

/// Every account in `/etc/passwd`, by uid.
pub fn user_names() -> HashMap<u32, Arc<str>> {
    let text = std::fs::read_to_string(PASSWD).unwrap_or_default();
    passwd_entries(&text)
        .map(|(uid, name)| (uid, Arc::from(name)))
        .collect()
}

pub(crate) fn user_name(uid: u32) -> Option<String> {
    let text = std::fs::read_to_string(PASSWD).ok()?;
    passwd_entries(&text)
        .find(|&(entry_uid, _)| entry_uid == uid)
        .map(|(_, name)| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "root:x:0:0::/root:/usr/bin/bash\n\
        bin:x:1:1::/:/usr/bin/nologin\n\
        marvelxcodes:x:1000:1000::/home/marvelxcodes:/usr/bin/fish\n";

    #[test]
    fn parses_entries() {
        let entries: Vec<_> = passwd_entries(FIXTURE).collect();
        assert_eq!(entries[0], (0, "root"));
        assert_eq!(entries[2], (1000, "marvelxcodes"));
    }

    #[test]
    fn resolves_root() {
        assert_eq!(user_name(0).as_deref(), Some("root"));
    }
}
