pub(crate) mod cgroup;
pub(crate) mod desktop;
mod registry;
pub(crate) mod resolver;
mod role;
mod row;
pub(crate) mod scope;
mod sort;

pub(crate) use registry::{AppRegistry, MemberFigures};
pub use role::{ProcessRole, RoleBreakdown};
pub use row::{AppRow, MemoryMethod};
pub use sort::{AppSortKey, Direction, ProcessSortKey, sort_apps, sort_processes_of_app};
