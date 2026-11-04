use crate::{Elapsed, FdIndex, Interest, SourceResult, Tick};

/// Everything a collector may consult during one sampling pass.
pub struct SampleContext<'a> {
    pub tick: Tick,
    pub elapsed: Elapsed,
    pub interest: &'a Interest,
    pub fds: &'a FdIndex,
}

/// A source of one domain's figures.
///
/// `sample` writes into an output that survives between ticks, so a
/// collector reuses its vectors instead of allocating new ones each pass.
pub trait Collector {
    type Output: Default + Clone;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut Self::Output,
    ) -> SourceResult<()>;
}
