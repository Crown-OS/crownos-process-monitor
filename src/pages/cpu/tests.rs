use crowntest::text;

use crate::app::live::fixture::{Swap, mount, seeded_live, shows};

#[test]
fn every_cpu_section_renders_with_seeded_data() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    for section in [
        "Hardware",
        "Utilization",
        "Apps",
        "Load and activity",
        "Load",
        "Pressure",
        "Context switches",
        "Running tasks",
        "Frequency and scaling",
        "Sensors and fans",
    ] {
        assert!(shows(&mut app, text(section)), "{section} is missing");
    }
    for removed in ["States", "Cores", "Average today", "Peak in window"] {
        assert!(!shows(&mut app, text(removed)), "{removed} is still shown");
    }
}

#[test]
fn hardware_sensors_and_fans_are_described() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    assert!(shows(&mut app, text("AMD Ryzen 7 7840HS")));
    assert!(shows(&mut app, text("L3 16.00 MB")));
    assert!(shows(&mut app, text("k10temp Tccd1")));
    assert!(shows(&mut app, text("66 °C")));
    assert!(shows(&mut app, text(super::fan_control::FAN_HELPER_NOTE)));
    assert!(shows(&mut app, text("Top App")));
    assert!(shows(&mut app, text("Firefox")));
}

#[test]
fn fan_control_opens_when_the_helper_writes_pwm() {
    use monitor_engine::helper::{Capability, HelperAvailability};

    let mut live = seeded_live(Swap::Configured);
    let mut tick = (*live.tick).clone();
    tick.helper = monitor_engine::HelperStatus {
        availability: HelperAvailability::Available,
        capabilities: std::sync::Arc::from([Capability::ControlFans]),
    };
    live.tick = std::sync::Arc::new(tick);
    let mut app = mount(live, super::page);
    app.settle();
    assert!(shows(&mut app, text("Fixed speed")));
    assert!(!shows(&mut app, text(super::fan_control::FAN_HELPER_NOTE)));
}

#[test]
fn an_empty_machine_shows_unavailable_states() {
    let mut app = mount(Default::default(), super::page);
    app.settle();
    assert!(shows(&mut app, text("No CPU temperature sensor was found")));
    assert!(shows(&mut app, text("No fan speeds are reported")));
}
