use crownui::kit::{
    ButtonOptions, ButtonVariant, TextInputOptions, ToggleItem, button, select, text_input,
    toggle_group, use_theme,
};
use crownui::prelude::{
    Cx, Listeners, Semantic, Signal, Styled, View, bind, hstack, keyed, spacer, text, vstack,
};

use crate::app::EngineLink;
use crate::settings::insights::{InsightSettings, LimitValue, SavedApp, SavedLimit};
use crate::util::format::{write_bytes, write_percent};

const LIMIT_MINUTES: [u32; 4] = [1, 5, 15, 30];
const DEFAULT_MINUTES_INDEX: usize = 1;
const GIBIBYTE: f64 = (1_u64 << 30) as f64;
const MAX_CPU_PERCENT: f32 = 100.0;
const ROW_GAP: f32 = 8.0;
const CAPTION_SIZE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitMetric {
    Cpu,
    Memory,
}

impl LimitMetric {
    const ALL: [Self; 2] = [Self::Cpu, Self::Memory];

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Cpu)
    }

    /// "80" means 80 % of the machine for CPU and 80 GB for memory.
    fn parse(self, typed: &str) -> Option<LimitValue> {
        let amount: f64 = typed
            .trim()
            .trim_end_matches(['%', 'G', 'B', ' '])
            .parse()
            .ok()?;
        if !amount.is_finite() || amount <= 0.0 {
            return None;
        }
        match self {
            Self::Cpu => {
                (amount as f32 <= MAX_CPU_PERCENT).then_some(LimitValue::CpuPercent(amount as f32))
            }
            Self::Memory => Some(LimitValue::MemoryBytes((amount * GIBIBYTE) as u64)),
        }
    }
}

/// "Firefox: memory over 4.00 GB for 5 min".
pub fn describe_limit(limit: &SavedLimit) -> String {
    let mut out = format!("{}: ", limit.app.name);
    match limit.value {
        LimitValue::CpuPercent(percent) => {
            out.push_str("CPU over ");
            write_percent(&mut out, percent);
        }
        LimitValue::MemoryBytes(bytes) => {
            out.push_str("memory over ");
            write_bytes(&mut out, bytes);
        }
    }
    out.push_str(&format!(" for {} min", limit.minutes));
    out
}

/// "Tell me when…": the saved per-app limits and a row to add one for a running app.
pub fn custom_limits(cx: &mut Cx, insights: Signal<InsightSettings>) -> impl View + use<> {
    let theme = use_theme(cx);
    let apps = EngineLink::find(cx).map_or_else(Vec::new, |link| link.running_apps());
    let app_names = cx.signal(
        apps.iter()
            .map(|(_, name)| name.to_string())
            .collect::<Vec<_>>(),
    );
    let picked_app = cx.signal(None::<usize>);
    let metric = cx.signal(0);
    let amount = cx.signal(String::new());
    let minutes = cx.signal(DEFAULT_MINUTES_INDEX);
    let problem = cx.signal(String::new());
    let saved = keyed(
        move |runtime| insights.with(runtime, |insights| insights.custom_limits.clone()),
        move |_, limits| {
            let rows: Vec<_> = limits
                .iter()
                .enumerate()
                .map(|(index, limit)| {
                    hstack((
                        text(describe_limit(limit)).text_size(CAPTION_SIZE),
                        spacer(),
                        button("Remove")
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |cx, _| {
                                insights.update(cx, |insights| {
                                    if index < insights.custom_limits.len() {
                                        insights.custom_limits.remove(index);
                                    }
                                });
                            }),
                    ))
                    .items_center()
                })
                .collect();
            vstack(rows).gap(ROW_GAP)
        },
    );
    let add = move |cx: &mut Cx| {
        let picked = picked_app.get(cx).and_then(|index| apps.get(index));
        let chosen = LimitMetric::from_index(metric.get(cx));
        let value = amount.with(cx, |typed| chosen.parse(typed));
        let (Some((key, name)), Some(value)) = (picked, value) else {
            problem.set(
                cx,
                "Pick an app and type a number (percent, or GB of memory)".to_owned(),
            );
            return;
        };
        let limit = SavedLimit {
            app: SavedApp::new(key, name),
            value,
            minutes: LIMIT_MINUTES[minutes.get(cx).min(LIMIT_MINUTES.len() - 1)],
        };
        insights.update(cx, |insights| insights.custom_limits.push(limit));
        amount.set(cx, String::new());
        problem.set(cx, String::new());
    };
    vstack((
        text("Tell me when…").font_semibold(),
        saved,
        hstack((
            select(app_names, picked_app),
            toggle_group(
                [ToggleItem::text("CPU %"), ToggleItem::text("Memory GB")],
                metric,
            ),
            text_input(amount)
                .placeholder("Over")
                .width(72.0)
                .label("Limit"),
        ))
        .gap(ROW_GAP)
        .items_center(),
        hstack((
            toggle_group(
                LIMIT_MINUTES.map(|minutes| ToggleItem::text(format!("for {minutes} min"))),
                minutes,
            ),
            spacer(),
            button("Add limit")
                .variant(ButtonVariant::Secondary)
                .on_click(move |cx, _| add(cx)),
        ))
        .gap(ROW_GAP)
        .items_center(),
        text(problem)
            .text_size(CAPTION_SIZE)
            .text_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.status.danger)
            })),
    ))
    .gap(ROW_GAP)
    .py(ROW_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_limits_parse_per_metric() {
        assert_eq!(
            LimitMetric::Cpu.parse("80"),
            Some(LimitValue::CpuPercent(80.0))
        );
        assert_eq!(
            LimitMetric::Cpu.parse("80 %"),
            Some(LimitValue::CpuPercent(80.0))
        );
        assert_eq!(LimitMetric::Cpu.parse("180"), None);
        assert_eq!(
            LimitMetric::Memory.parse("4 GB"),
            Some(LimitValue::MemoryBytes(4 << 30))
        );
        assert_eq!(LimitMetric::Memory.parse("-1"), None);
        assert_eq!(LimitMetric::Memory.parse("lots"), None);
    }

    #[test]
    fn limits_read_as_a_sentence() {
        let limit = SavedLimit {
            app: SavedApp {
                key: "desktop:firefox".to_owned(),
                name: "Firefox".to_owned(),
            },
            value: LimitValue::MemoryBytes(4 << 30),
            minutes: 5,
        };
        assert_eq!(
            describe_limit(&limit),
            "Firefox: memory over 4.00 GB for 5 min"
        );
    }
}
