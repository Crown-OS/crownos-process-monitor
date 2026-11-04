use super::data::RowRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Start,
    End,
}

/// Whether the Columns menu can hide a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Always,
    Optional,
}

/// Writes a cell's text for one row.
pub type CellText<D> = fn(&D, RowRef, &mut String);
/// A row's numeric sort key in this column.
pub type SortKey<D> = fn(&D, RowRef) -> f64;

/// One metric column after the built-in Name column. Columns are plain data, so a page can
/// declare its set as a `const` slice.
pub struct Column<D: ?Sized> {
    pub id: &'static str,
    pub title: &'static str,
    pub width: f32,
    pub alignment: Alignment,
    pub visibility: Visibility,
    pub text: CellText<D>,
    pub sort_key: Option<SortKey<D>>,
}

impl<D: ?Sized> Clone for Column<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: ?Sized> Copy for Column<D> {}

impl<D: ?Sized> Column<D> {
    /// A right-aligned, always visible, sortable number column.
    pub const fn metric(
        id: &'static str,
        title: &'static str,
        width: f32,
        text: CellText<D>,
        sort_key: SortKey<D>,
    ) -> Self {
        Self {
            id,
            title,
            width,
            alignment: Alignment::End,
            visibility: Visibility::Always,
            text,
            sort_key: Some(sort_key),
        }
    }

    /// A left-aligned detail column (user, path, state) the Columns menu can show.
    pub const fn detail(
        id: &'static str,
        title: &'static str,
        width: f32,
        text: CellText<D>,
    ) -> Self {
        Self {
            id,
            title,
            width,
            alignment: Alignment::Start,
            visibility: Visibility::Optional,
            text,
            sort_key: None,
        }
    }

    #[must_use]
    pub const fn optional(self) -> Self {
        Self {
            visibility: Visibility::Optional,
            ..self
        }
    }

    #[must_use]
    pub const fn sorted_by(self, sort_key: SortKey<D>) -> Self {
        Self {
            sort_key: Some(sort_key),
            ..self
        }
    }

    pub fn is_shown(&self, index: usize, visible: ColumnSet) -> bool {
        self.visibility == Visibility::Always || visible.contains(index)
    }
}

/// Which optional columns are shown, by index into the column list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColumnSet(u64);

impl ColumnSet {
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn with(self, index: usize) -> Self {
        Self(self.0 | 1 << index)
    }

    pub const fn contains(self, index: usize) -> bool {
        self.0 & (1 << index) != 0
    }

    #[must_use]
    pub const fn toggled(self, index: usize) -> Self {
        Self(self.0 ^ 1 << index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_sets_toggle_single_columns() {
        let set = ColumnSet::NONE.with(2);
        assert!(set.contains(2));
        assert!(!set.contains(1));
        assert!(!set.toggled(2).contains(2));
        assert!(set.toggled(0).contains(0));
    }
}
