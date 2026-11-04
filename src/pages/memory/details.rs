use crownui::kit::icons;
use crownui::prelude::{AnyView, Cx, Runtime, Signal, View, bind, keyed};
use monitor_engine::Snapshot;
use monitor_engine::system::{HugePages, ZramDevice};

use super::super::sources::names_key;
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::empty_state::empty_state;
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::{bytes, count};

fn tick_fact(
    live: Signal<LiveSnapshot>,
    label: impl Into<String>,
    describe: impl Fn(&Snapshot) -> String + 'static,
) -> Fact {
    Fact::new(
        label,
        bind(move |runtime: &mut Runtime| live.with(runtime, |snapshot| describe(&snapshot.tick))),
    )
}

fn per_second(value: f32) -> String {
    format!("{}/s", count(value.max(0.0).round() as u64))
}

/// "4 of 16 free · 1 reserved · 2 MB pages", or "None reserved".
pub fn describe_huge_pages(pages: &HugePages) -> String {
    if pages.total == 0 {
        return "None reserved".to_owned();
    }
    format!(
        "{} of {} free · {} reserved · {} pages",
        pages.free,
        pages.total,
        pages.reserved,
        bytes(pages.page_size.get())
    )
}

/// "1.00 GB → 256 MB · 4.0× · zstd".
pub fn describe_zram(device: &ZramDevice) -> String {
    let mut out = format!(
        "{} → {} · {:.1}×",
        bytes(device.original.get()),
        bytes(device.compressed.get()),
        device.ratio()
    );
    if let Some(algorithm) = &device.algorithm {
        out.push_str(" · ");
        out.push_str(algorithm);
    }
    out
}

/// Faults, OOM kills, dirty pages, huge pages and locked memory.
pub fn activity_facts(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    fact_grid(vec![
        tick_fact(live, "Page faults (minor · major)", |tick| {
            let memory = &tick.memory;
            format!(
                "{} · {}",
                per_second(memory.minor_fault_rate),
                per_second(memory.major_fault_rate)
            )
        }),
        tick_fact(live, "OOM kills since boot", |tick| {
            count(tick.memory.oom_kills)
        }),
        tick_fact(live, "Dirty · writeback", |tick| {
            format!(
                "{} · {}",
                bytes(tick.memory.dirty.get()),
                bytes(tick.memory.writeback.get())
            )
        }),
        tick_fact(live, "Transparent huge pages", |tick| {
            bytes(tick.memory.huge_pages.transparent.get())
        }),
        tick_fact(live, "Huge pages", |tick| {
            describe_huge_pages(&tick.memory.huge_pages)
        }),
        tick_fact(live, "Mlocked · unevictable", |tick| {
            format!(
                "{} · {}",
                bytes(tick.memory.locked.get()),
                bytes(tick.memory.unevictable.get())
            )
        }),
        tick_fact(live, "Vmalloc", |tick| {
            bytes(tick.memory.vmalloc_used.get())
        }),
    ])
}

fn zram_names(snapshot: &LiveSnapshot) -> (u64, bool) {
    let memory = &snapshot.tick.memory;
    (
        names_key(memory.zram.iter().map(|device| &*device.name)),
        memory.zswap.is_some(),
    )
}

/// zram devices (original against compressed, ratio, algorithm) and the zswap pool.
pub fn compression_panel(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    keyed(
        move |runtime| live.with(runtime, zram_names),
        move |cx, (_, zswap)| -> AnyView {
            let names: Vec<String> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .tick
                    .memory
                    .zram
                    .iter()
                    .map(|device| device.name.to_string())
                    .collect()
            });
            if names.is_empty() && !zswap {
                return empty_state(
                    icons::ARCHIVE,
                    "No memory compression",
                    "Neither zram nor zswap is in use.",
                )
                .boxed();
            }
            let mut facts: Vec<_> = names
                .into_iter()
                .enumerate()
                .map(|(index, name)| {
                    tick_fact(live, name, move |tick| {
                        tick.memory
                            .zram
                            .get(index)
                            .map_or_else(String::new, describe_zram)
                    })
                })
                .collect();
            if zswap {
                facts.push(tick_fact(live, "zswap pool · stored", |tick| {
                    tick.memory.zswap.map_or_else(String::new, |zswap| {
                        format!(
                            "{} · {}",
                            bytes(zswap.pool.get()),
                            bytes(zswap.stored.get())
                        )
                    })
                }));
            }
            fact_grid(facts).boxed()
        },
    )
}

#[cfg(test)]
mod tests {
    use monitor_core::Bytes;

    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn zram_reads_as_a_ratio() {
        let device = ZramDevice {
            name: "zram0".into(),
            original: Bytes::new(GIB),
            compressed: Bytes::new(GIB / 4),
            algorithm: Some("zstd".into()),
            ..ZramDevice::default()
        };
        assert_eq!(describe_zram(&device), "1.00 GB → 256 MB · 4.0× · zstd");
    }

    #[test]
    fn no_huge_pages_reads_plainly() {
        assert_eq!(describe_huge_pages(&HugePages::default()), "None reserved");
    }
}
