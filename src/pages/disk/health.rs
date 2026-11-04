use std::sync::Arc;
use std::time::Duration;

use crownui::kit::{
    ButtonOptions, ButtonSize, ButtonVariant, StatusOptions, Tone, button, icons, status,
};
use crownui::prelude::{
    AnyView, Listeners, Role, Semantic, Styled, View, hstack, keyed, spacer, text, vstack,
};
use monitor_engine::Command;
use monitor_engine::io::{AtaHealth, DriveHealth, HealthVerdict, NvmeHealth};

use crate::app::{AppState, EngineLink};
use crate::components::fact_row::fact_row;
use crate::util::format::{bytes, count, duration, percent};
use crate::util::spacing;

const TITLE_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 12.0;
const SHOWN_ATTRIBUTES: usize = 12;
const SECONDS_PER_HOUR: u64 = 3_600;

pub const fn verdict_badge(verdict: HealthVerdict) -> (&'static str, Tone) {
    match verdict {
        HealthVerdict::Good => ("Healthy", Tone::Success),
        HealthVerdict::Warning => ("Needs attention", Tone::Warning),
        HealthVerdict::Failing => ("Failing", Tone::Danger),
        HealthVerdict::Unknown => ("Unknown", Tone::Neutral),
    }
}

fn nvme_rows(nvme: &NvmeHealth) -> Vec<AnyView> {
    let optional = |value: Option<String>| value.unwrap_or_else(|| "–".to_owned());
    let mut rows = vec![
        fact_row(
            "Percentage used",
            optional(nvme.percent_used.map(|used| percent(used.get()))),
        )
        .boxed(),
        fact_row(
            "Available spare",
            optional(nvme.available_spare.map(|spare| {
                let threshold = nvme.spare_threshold.map_or_else(String::new, |threshold| {
                    format!(" (warns below {})", percent(threshold.get()))
                });
                format!("{}{threshold}", percent(spare.get()))
            })),
        )
        .boxed(),
        fact_row("Media errors", optional(nvme.media_errors.map(count))).boxed(),
        fact_row(
            "Powered on",
            optional(
                nvme.power_on_hours
                    .map(|hours| duration(hours * SECONDS_PER_HOUR)),
            ),
        )
        .boxed(),
        fact_row(
            "Data written",
            optional(nvme.data_written.map(|data| bytes(data.get()))),
        )
        .boxed(),
        fact_row(
            "Data read",
            optional(nvme.data_read.map(|data| bytes(data.get()))),
        )
        .boxed(),
        fact_row(
            "Unsafe shutdowns",
            optional(nvme.unsafe_shutdowns.map(count)),
        )
        .boxed(),
    ];
    if !nvme.critical_warnings.is_empty() {
        let warnings: Vec<&str> = nvme
            .critical_warnings
            .iter()
            .map(|warning| &**warning)
            .collect();
        rows.push(fact_row("Critical warnings", warnings.join(", ")).boxed());
    }
    rows.extend(
        nvme.selftest_status
            .as_deref()
            .map(|status| fact_row("Self-test", status.to_owned()).boxed()),
    );
    rows
}

fn ata_rows(ata: &AtaHealth) -> Vec<AnyView> {
    if !ata.supported || !ata.enabled {
        return vec![
            text("SMART is off or not supported on this drive.")
                .text_size(DETAIL_SIZE)
                .boxed(),
        ];
    }
    let mut rows = vec![
        fact_row(
            "Powered on",
            ata.power_on
                .map_or_else(|| "–".to_owned(), |time: Duration| duration(time.as_secs())),
        )
        .boxed(),
        fact_row(
            "Bad sectors",
            ata.bad_sectors.map_or_else(|| "–".to_owned(), count),
        )
        .boxed(),
    ];
    rows.extend(
        ata.selftest_status
            .as_deref()
            .map(|status| fact_row("Self-test", status.to_owned()).boxed()),
    );
    rows.extend(
        ata.attributes
            .iter()
            .take(SHOWN_ATTRIBUTES)
            .map(|attribute| {
                let normalized = match (attribute.value, attribute.threshold) {
                    (Some(value), Some(threshold)) => format!("{value} (threshold {threshold})"),
                    (Some(value), None) => value.to_string(),
                    _ => "–".to_owned(),
                };
                let failing = if attribute.is_failing() {
                    " · failing"
                } else {
                    ""
                };
                text(format!(
                    "{:>3} {} · {normalized} · raw {}{failing}",
                    attribute.id, attribute.name, attribute.pretty
                ))
                .text_size(DETAIL_SIZE)
                .boxed()
            }),
    );
    rows
}

fn health_body(health: &DriveHealth) -> Vec<AnyView> {
    match health {
        DriveHealth::Pending => vec![text("Waiting for UDisks2…").text_size(DETAIL_SIZE).boxed()],
        DriveHealth::Unavailable => vec![
            text("This drive does not report SMART, or UDisks2 is not running.")
                .text_size(DETAIL_SIZE)
                .boxed(),
        ],
        DriveHealth::Nvme(nvme) => nvme_rows(nvme),
        DriveHealth::Ata(ata) => ata_rows(ata),
    }
}

/// Which health reading a device shows, so the card rebuilds only on a new one.
fn health_identity(health: &DriveHealth) -> usize {
    match health {
        DriveHealth::Pending => 0,
        DriveHealth::Unavailable => 1,
        DriveHealth::Ata(ata) => Arc::as_ptr(ata) as usize,
        DriveHealth::Nvme(nvme) => Arc::as_ptr(nvme) as usize,
    }
}

/// SMART status and figures for one disk, with Refresh asking UDisks2 again.
pub fn health_card(state: AppState, index: usize, link: Option<EngineLink>) -> impl View {
    let live = state.live;
    let refresh = button("Refresh")
        .variant(ButtonVariant::Ghost)
        .control_size(ButtonSize::Small)
        .leading_icon(icons::REFRESH_CW)
        .disabled(link.is_none())
        .on_click(move |_, _| {
            if let Some(link) = &link {
                link.send(Command::RefreshDriveHealth);
            }
        });
    let body = keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot
                    .tick
                    .disk
                    .devices
                    .get(index)
                    .map_or(0, |device| health_identity(&device.health))
            })
        },
        move |cx, _| {
            let (verdict, rows) = live.with_untracked(cx, |snapshot| {
                snapshot.tick.disk.devices.get(index).map_or_else(
                    || (HealthVerdict::Unknown, Vec::new()),
                    |device| (device.health.verdict(), health_body(&device.health)),
                )
            });
            let (label, tone) = verdict_badge(verdict);
            vstack((
                hstack(status(label).tone(tone)),
                vstack(rows).gap(spacing::TIGHT).w_full(),
            ))
            .gap(spacing::SNUG)
            .w_full()
        },
    );
    vstack((
        hstack((
            text("Health").text_size(TITLE_SIZE).font_semibold(),
            spacer(),
            refresh,
        ))
        .items_center(),
        body,
    ))
    .gap(spacing::SNUG)
    .w_full()
    .role(Role::Group)
    .label("Health")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_pick_their_tone() {
        assert_eq!(verdict_badge(HealthVerdict::Failing).1, Tone::Danger);
        assert_eq!(verdict_badge(HealthVerdict::Good).0, "Healthy");
    }
}
