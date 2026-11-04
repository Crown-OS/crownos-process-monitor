use crownui::ext::Prop;
use crownui::prelude::Runtime;

/// The current value of `prop`, subscribing the running observer when it is dynamic.
pub fn read_prop<T: Clone>(prop: &mut Prop<T>, runtime: &mut Runtime) -> T {
    match prop {
        Prop::Static(value) => value.clone(),
        Prop::Dynamic(compute) => compute(runtime),
    }
}
