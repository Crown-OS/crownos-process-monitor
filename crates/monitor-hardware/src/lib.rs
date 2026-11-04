//! Hardware collectors: GPUs and NPUs, hwmon sensors and fans, batteries,
//! power draw, power profiles and sleep inhibitors.

pub mod gpu;
pub mod power;
pub mod sensors;
mod util;

pub use gpu::{
    AcceleratorSnapshot, BackendKind, ClockEventReasons, DriverSupport, EngineClass, EngineUsage,
    GpuCollector, GpuDevice, GpuIdentity, GpuKind, GpuMemory, GpuMetrics, GpuPower,
    GpuProcessUsage, GpuSnapshot, GpuVendor, PerformanceState, wants_drm_fds,
};
pub use power::{
    BatteryReading, BatteryStatus, EnergyCounter, Inhibitor, PowerCollector, PowerSnapshot,
    PowerSource, RaplReader, SystemDraw, estimate_system_draw,
};
pub use sensors::{
    FanControl, FanControlError, FanId, FanReading, SensorCategory, SensorCollector,
    SensorSnapshot, TemperatureReading,
};
