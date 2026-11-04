use regex_lite::{Regex, RegexBuilder};

/// What the find field filters by: a case-insensitive substring, or `/regex/`.
#[derive(Debug, Clone, Default)]
pub enum FindQuery {
    #[default]
    Everything,
    Substring(String),
    Pattern(Regex),
}

impl FindQuery {
    /// Compiles `text` once; an invalid `/regex/` falls back to matching it literally.
    pub fn parse(text: &str) -> Self {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Self::Everything;
        }
        let pattern = trimmed
            .strip_prefix('/')
            .and_then(|rest| rest.strip_suffix('/'))
            .filter(|pattern| !pattern.is_empty());
        if let Some(Ok(regex)) =
            pattern.map(|pattern| RegexBuilder::new(pattern).case_insensitive(true).build())
        {
            return Self::Pattern(regex);
        }
        Self::Substring(trimmed.to_ascii_lowercase())
    }

    pub fn is_everything(&self) -> bool {
        matches!(self, Self::Everything)
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Substring(needle) => contains_ignoring_ascii_case(name, needle),
            Self::Pattern(regex) => regex.is_match(name),
        }
    }
}

fn contains_ignoring_ascii_case(haystack: &str, lowercase_needle: &str) -> bool {
    let needle = lowercase_needle.as_bytes();
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substrings_ignore_case() {
        let query = FindQuery::parse("  FIRE ");
        assert!(query.matches("Firefox"));
        assert!(!query.matches("Files"));
        assert!(FindQuery::parse("").is_everything());
    }

    #[test]
    fn slashes_compile_a_regex() {
        let query = FindQuery::parse("/^f(ire|il)/");
        assert!(matches!(query, FindQuery::Pattern(_)));
        assert!(query.matches("Firefox"));
        assert!(query.matches("files"));
        assert!(!query.matches("Code"));
    }

    #[test]
    fn invalid_patterns_match_literally() {
        let query = FindQuery::parse("/fo(/");
        assert!(matches!(query, FindQuery::Substring(_)));
        assert!(query.matches("x/fo(/y"));
    }
}
