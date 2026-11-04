/// Which group of the table an app belongs to, for the filter chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppCategory {
    /// The user's own apps and processes.
    #[default]
    Mine,
    /// Services, kernel threads and other users' processes.
    System,
    Container,
    /// The "Ended recently" group, always listed last.
    Ended,
}

/// The filter chips above the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppFilter {
    MyApps,
    #[default]
    All,
    System,
    Containers,
}

impl AppFilter {
    pub const ALL: [Self; 4] = [Self::MyApps, Self::All, Self::System, Self::Containers];

    pub const fn label(self) -> &'static str {
        match self {
            Self::MyApps => "My apps",
            Self::All => "All",
            Self::System => "System",
            Self::Containers => "Containers",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::MyApps => 0,
            Self::All => 1,
            Self::System => 2,
            Self::Containers => 3,
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    /// Ended processes are the user's recent history, so My apps keeps them too.
    pub const fn admits(self, category: AppCategory) -> bool {
        matches!(
            (self, category),
            (Self::All, _)
                | (Self::MyApps, AppCategory::Mine | AppCategory::Ended)
                | (Self::System, AppCategory::System)
                | (Self::Containers, AppCategory::Container)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_admit_their_categories() {
        assert!(AppFilter::All.admits(AppCategory::System));
        assert!(AppFilter::MyApps.admits(AppCategory::Mine));
        assert!(AppFilter::MyApps.admits(AppCategory::Ended));
        assert!(!AppFilter::MyApps.admits(AppCategory::Container));
        assert!(AppFilter::System.admits(AppCategory::System));
        assert!(!AppFilter::System.admits(AppCategory::Mine));
        assert!(AppFilter::Containers.admits(AppCategory::Container));
        assert!(!AppFilter::Containers.admits(AppCategory::Ended));
    }

    #[test]
    fn chip_indices_round_trip() {
        for filter in AppFilter::ALL {
            assert_eq!(AppFilter::from_index(filter.index()), filter);
        }
        assert_eq!(AppFilter::from_index(99), AppFilter::All);
    }
}
