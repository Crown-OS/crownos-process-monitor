use super::column::{Column, SortKey};
use super::data::{RowId, RowRef, TableRows};
use super::filter::{AppCategory, AppFilter};
use super::find::FindQuery;
use super::sort::{SortColumn, SortDirection, SortOrder, compare_names};

/// What narrows and orders the rows besides the data itself.
pub struct RowFilter<'a> {
    pub query: &'a FindQuery,
    pub sort: SortOrder,
    pub expanded: &'a [u64],
    pub pinned: &'a [u64],
    pub chip: AppFilter,
}

/// The flattened, sorted, filtered row list the virtual list indexes into. Rebuilt only when
/// the data, sort, filter or expansion changes; its buffers are reused across rebuilds.
#[derive(Debug, Default)]
pub struct RowModel {
    rows: Vec<RowRef>,
    apps: Vec<usize>,
    processes: Vec<usize>,
}

impl RowModel {
    pub fn rows(&self) -> &[RowRef] {
        &self.rows
    }

    pub fn position<D: TableRows + ?Sized>(&self, data: &D, id: RowId) -> Option<usize> {
        self.rows.iter().position(|row| data.row_id(*row) == id)
    }

    pub fn rebuild<D: TableRows + ?Sized>(
        &mut self,
        data: &D,
        columns: &[Column<D>],
        filter: &RowFilter<'_>,
    ) {
        let key: Option<SortKey<D>> = match filter.sort.column {
            SortColumn::Name => None,
            SortColumn::Metric(index) => columns.get(index).and_then(|column| column.sort_key),
        };
        let direction = filter.sort.direction;
        let compare = |first: RowRef, second: RowRef| {
            let ordering = match key {
                Some(key) => key(data, first).total_cmp(&key(data, second)),
                None => compare_names(data.name(first), data.name(second)),
            };
            match direction {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        };
        let query = filter.query;
        let process_matches =
            |app: usize, process: usize| query.matches(data.name(RowRef::Process(app, process)));
        let app_matches = |app: usize| query.matches(data.name(RowRef::App(app)));

        self.apps.clear();
        self.apps.extend((0..data.app_count()).filter(|app| {
            filter.chip.admits(data.category(*app))
                && (app_matches(*app)
                    || (0..data.process_count(*app)).any(|process| process_matches(*app, process)))
        }));
        let pinned = |app: usize| filter.pinned.contains(&data.app_key(app));
        let ended = |app: usize| data.category(app) == AppCategory::Ended;
        self.apps.sort_by(|first, second| {
            ended(*first)
                .cmp(&ended(*second))
                .then_with(|| pinned(*second).cmp(&pinned(*first)))
                .then_with(|| compare(RowRef::App(*first), RowRef::App(*second)))
        });

        self.rows.clear();
        for &app in &self.apps {
            self.rows.push(RowRef::App(app));
            let only_matches = !query.is_everything() && !app_matches(app);
            if !only_matches && !filter.expanded.contains(&data.app_key(app)) {
                continue;
            }
            self.processes.clear();
            self.processes.extend(
                (0..data.process_count(app))
                    .filter(|process| !only_matches || process_matches(app, *process)),
            );
            self.processes.sort_by(|first, second| {
                compare(RowRef::Process(app, *first), RowRef::Process(app, *second))
            });
            self.rows.extend(
                self.processes
                    .iter()
                    .map(|process| RowRef::Process(app, *process)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::app_table::data::RowMark;

    type FixtureApp = (&'static str, f64, Vec<(&'static str, f64)>);

    struct Fixture {
        apps: Vec<FixtureApp>,
        categories: Vec<AppCategory>,
    }

    impl TableRows for Fixture {
        fn app_count(&self) -> usize {
            self.apps.len()
        }
        fn process_count(&self, app: usize) -> usize {
            self.apps[app].2.len()
        }
        fn app_key(&self, app: usize) -> u64 {
            app as u64 + 1
        }
        fn process_key(&self, app: usize, process: usize) -> u64 {
            (app * 100 + process) as u64
        }
        fn name(&self, row: RowRef) -> &str {
            match row {
                RowRef::App(app) => self.apps[app].0,
                RowRef::Process(app, process) => self.apps[app].2[process].0,
            }
        }
        fn icon(&self, _: usize) -> &'static str {
            ""
        }
        fn mark(&self, _: RowRef) -> RowMark {
            RowMark::None
        }
        fn category(&self, app: usize) -> AppCategory {
            self.categories.get(app).copied().unwrap_or_default()
        }
    }

    fn cpu(data: &Fixture, row: RowRef) -> f64 {
        match row {
            RowRef::App(app) => data.apps[app].1,
            RowRef::Process(app, process) => data.apps[app].2[process].1,
        }
    }

    const COLUMNS: [Column<Fixture>; 1] = [Column::metric("cpu", "CPU", 60.0, |_, _, _| {}, cpu)];

    fn fixture() -> Fixture {
        Fixture {
            apps: vec![
                (
                    "Firefox",
                    30.0,
                    vec![("firefox", 10.0), ("Web Content", 20.0)],
                ),
                ("Code", 50.0, vec![("code", 50.0)]),
                ("bash", 1.0, vec![("bash", 1.0)]),
            ],
            categories: Vec::new(),
        }
    }

    fn rows(
        data: &Fixture,
        query: &str,
        sort: SortOrder,
        expanded: &[u64],
        pinned: &[u64],
    ) -> Vec<RowRef> {
        rows_through(data, query, sort, (expanded, pinned), AppFilter::All)
    }

    fn rows_through(
        data: &Fixture,
        query: &str,
        sort: SortOrder,
        (expanded, pinned): (&[u64], &[u64]),
        chip: AppFilter,
    ) -> Vec<RowRef> {
        let mut model = RowModel::default();
        let query = FindQuery::parse(query);
        model.rebuild(
            data,
            &COLUMNS,
            &RowFilter {
                query: &query,
                sort,
                expanded,
                pinned,
                chip,
            },
        );
        model.rows().to_vec()
    }

    const BY_CPU: SortOrder = SortOrder::new(SortColumn::Metric(0), SortDirection::Descending);
    const BY_NAME: SortOrder = SortOrder::new(SortColumn::Name, SortDirection::Ascending);

    #[test]
    fn apps_sort_by_metric_or_by_name() {
        let data = fixture();
        assert_eq!(
            rows(&data, "", BY_CPU, &[], &[]),
            [RowRef::App(1), RowRef::App(0), RowRef::App(2)]
        );
        assert_eq!(
            rows(&data, "", BY_NAME, &[], &[]),
            [RowRef::App(2), RowRef::App(1), RowRef::App(0)]
        );
    }

    #[test]
    fn expanded_apps_list_their_processes_in_the_same_order() {
        let data = fixture();
        assert_eq!(
            rows(&data, "", BY_CPU, &[1], &[]),
            [
                RowRef::App(1),
                RowRef::App(0),
                RowRef::Process(0, 1),
                RowRef::Process(0, 0),
                RowRef::App(2),
            ]
        );
    }

    #[test]
    fn pinned_apps_stay_on_top() {
        let data = fixture();
        assert_eq!(rows(&data, "", BY_CPU, &[], &[3])[0], RowRef::App(2));
    }

    #[test]
    fn find_keeps_matching_apps_and_reveals_matching_processes() {
        let data = fixture();
        assert_eq!(rows(&data, "code", BY_CPU, &[], &[]), [RowRef::App(1)]);
        assert_eq!(
            rows(&data, "web", BY_CPU, &[], &[]),
            [RowRef::App(0), RowRef::Process(0, 1)]
        );
        assert_eq!(
            rows(&data, "/^(ba|co)/", BY_NAME, &[], &[]),
            [RowRef::App(2), RowRef::App(1)]
        );
    }

    #[test]
    fn the_ended_group_sorts_last_even_when_pinned() {
        let mut data = fixture();
        data.categories = vec![AppCategory::Ended, AppCategory::Mine, AppCategory::Mine];
        assert_eq!(
            rows(&data, "", BY_NAME, &[], &[1]),
            [RowRef::App(2), RowRef::App(1), RowRef::App(0)]
        );
        assert_eq!(
            rows(&data, "", BY_CPU, &[], &[]).last(),
            Some(&RowRef::App(0))
        );
    }

    #[test]
    fn chips_narrow_the_apps() {
        let mut data = fixture();
        data.categories = vec![
            AppCategory::Mine,
            AppCategory::System,
            AppCategory::Container,
        ];
        let all = (&[] as &[u64], &[] as &[u64]);
        assert_eq!(
            rows_through(&data, "", BY_NAME, all, AppFilter::System),
            [RowRef::App(1)]
        );
        assert_eq!(
            rows_through(&data, "", BY_NAME, all, AppFilter::Containers),
            [RowRef::App(2)]
        );
        assert_eq!(
            rows_through(&data, "", BY_NAME, all, AppFilter::MyApps),
            [RowRef::App(0)]
        );
    }

    #[test]
    fn positions_follow_row_identity() {
        let data = fixture();
        let mut model = RowModel::default();
        let query = FindQuery::Everything;
        model.rebuild(
            &data,
            &COLUMNS,
            &RowFilter {
                query: &query,
                sort: BY_CPU,
                expanded: &[],
                pinned: &[],
                chip: AppFilter::All,
            },
        );
        assert_eq!(model.position(&data, data.row_id(RowRef::App(2))), Some(2));
    }
}
