use crownui::prelude::{Cx, Memo, Runtime, Signal};

use super::choice::Choice;
use super::insights::InsightSettings;
use super::model::MonitorSettings;
use super::persist::{SettingsWriter, load_insights};

/// The live settings: one signal the app reads, saved whenever it changes, and the
/// list-shaped insights beside it, loaded off the UI thread.
#[derive(Clone, Copy)]
pub struct Settings {
    value: Signal<MonitorSettings>,
    insights: Signal<InsightSettings>,
}

impl Settings {
    pub fn new(cx: &mut Cx, initial: MonitorSettings, writer: Option<SettingsWriter>) -> Self {
        let value = cx.signal(initial);
        let insights = cx.signal(InsightSettings::default());
        if let Some(writer) = writer {
            let mut saved = initial;
            let saving = writer.clone();
            cx.effect(move |runtime| {
                let current = value.get(runtime);
                if current != saved {
                    saved = current;
                    saving.save(current);
                }
            });
            persist_insights(cx, insights, writer);
        }
        Self { value, insights }
    }

    /// Per-app limits and quieted alerts; defaults until the section has loaded.
    pub fn insights(self) -> Signal<InsightSettings> {
        self.insights
    }

    pub fn get(self, runtime: &mut Runtime) -> MonitorSettings {
        self.value.get(runtime)
    }

    pub fn current(self, runtime: &Runtime) -> MonitorSettings {
        self.value.with_untracked(runtime, |settings| *settings)
    }

    /// A memo of one field, so readers only wake when that field changes.
    pub fn field<T: PartialEq + 'static>(
        self,
        cx: &mut Cx,
        pick: fn(&MonitorSettings) -> T,
    ) -> Memo<T> {
        let value = self.value;
        cx.memo(move |runtime| value.with(runtime, pick))
    }

    /// A segmented-control index bound both ways to one field: picking an option writes the
    /// field, and a change made elsewhere moves the index.
    pub fn choice<C: Choice>(
        self,
        cx: &mut Cx,
        get: fn(&MonitorSettings) -> C,
        set: fn(&mut MonitorSettings, C),
    ) -> Signal<usize> {
        let value = self.value;
        let initial = value.with_untracked(cx, |settings| get(settings).index());
        let selected = cx.signal(initial);
        cx.effect(move |runtime| {
            let chosen = C::from_index(selected.get(runtime));
            if value.with_untracked(runtime, get) != chosen {
                value.update(runtime, |settings| set(settings, chosen));
            }
        });
        cx.effect(move |runtime| {
            let index = value.with(runtime, |settings| get(settings).index());
            if selected.with_untracked(runtime, |current| *current != index) {
                selected.set(runtime, index);
            }
        });
        selected
    }
}

/// Loads the insights section on a blocking task, then saves every later change.
fn persist_insights(cx: &mut Cx, insights: Signal<InsightSettings>, writer: SettingsWriter) {
    let saved = cx.store(InsightSettings::default());
    cx.effect(move |runtime| {
        insights.with(runtime, |_| ());
        let changed = insights.with_untracked(runtime, |current| {
            saved.with(runtime, |saved| current != saved)
        });
        if changed {
            let current = insights.with_untracked(runtime, InsightSettings::clone);
            saved.update(runtime, |saved| saved.clone_from(&current));
            writer.save_insights(current);
        }
    });
    let load = async { tokio::task::spawn_blocking(load_insights).await.ok() };
    cx.spawn(load, move |cx, loaded| {
        if let Some(loaded) = loaded {
            saved.update(cx, |saved| saved.clone_from(&loaded));
            insights.set(cx, loaded);
        }
    });
}
