//! Shared primitives for the CrownOS process monitor: identifiers, units,
//! clocks, ring buffers, the collector contract and allocation-free readers
//! for `/proc` and `/sys`.

pub mod app;
pub mod clock;
pub mod collector;
pub mod error;
pub mod fd_index;
pub mod ids;
pub mod interest;
pub mod pressure;
pub mod ring;
pub mod units;
pub mod util;

pub use app::{AppKey, AppKind};
pub use clock::{Elapsed, Tick};
pub use collector::{Collector, SampleContext};
pub use error::{SourceError, SourceResult};
pub use fd_index::{DrmFd, FdIndex, FdScope};
pub use ids::{DeviceKey, Pid, ProcessKey, Uid};
pub use interest::{Interest, Page, WindowState};
pub use pressure::{MemoryStall, PressureLevel};
pub use ring::RingSeries;
pub use units::{Bytes, BytesPerSecond, Celsius, Coverage, Megahertz, Percent, Rpm, Watts};
