//! A virtualized app table: one row per app, expandable into its processes, with sortable
//! columns, a Columns menu, find, selection, keyboard navigation and a context menu.

mod column;
mod data;
mod filter;
mod find;
mod header;
mod keys;
mod model;
mod row;
mod row_leading;
mod row_source;
mod sort;
mod state;
mod table;

pub use column::{Alignment, Column, ColumnSet, Visibility};
pub use data::{RowId, RowMark, RowRef, TableRows};
pub use filter::{AppCategory, AppFilter};
pub use find::FindQuery;
pub use model::RowModel;
pub use sort::{SortColumn, SortDirection, SortOrder};
pub use state::TableState;
pub use table::{AppTable, app_table};
