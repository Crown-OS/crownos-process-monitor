use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    /// An index into the table's columns.
    Metric(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortOrder {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl SortOrder {
    pub const fn new(column: SortColumn, direction: SortDirection) -> Self {
        Self { column, direction }
    }
}

pub(super) fn compare_names(first: &str, second: &str) -> Ordering {
    first
        .chars()
        .flat_map(char::to_lowercase)
        .cmp(second.chars().flat_map(char::to_lowercase))
}
