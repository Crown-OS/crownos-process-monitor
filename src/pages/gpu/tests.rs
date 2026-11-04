use crowntest::{Role, role, text};
use monitor_core::{DeviceKey, Percent};
use monitor_engine::hardware::{AcceleratorSnapshot, GpuPower};

use crate::app::live::fixture::{
    INTEGRATED_GPU, Swap, live_from, mount, seeded_live, seeded_tick, shows,
};

#[test]
fn every_gpu_section_renders_with_seeded_data() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    for section in ["Hardware", "Utilization", "VRAM", "Readings", "Apps"] {
        assert!(shows(&mut app, text(section)), "{section} is missing");
    }
    for removed in [
        "Video memory",
        "PCI ids",
        "VBIOS",
        "Read through",
        "Support",
    ] {
        assert!(!shows(&mut app, text(removed)), "{removed} is still shown");
    }
    for engine in [
        "Graphics",
        "Compute",
        "Copy",
        "Video decode",
        "Video encode",
        "Enhance",
    ] {
        assert!(shows(&mut app, text(engine)), "{engine} legend is missing");
        assert!(
            !shows(&mut app, role(Role::Button).name(engine)),
            "{engine} legend still toggles"
        );
    }
    assert!(shows(&mut app, text("amdgpu")), "the driver is listed");
    assert!(
        !shows(&mut app, text(INTEGRATED_GPU)),
        "no identity line under the selector"
    );
    assert!(shows(&mut app, text("1.80 GHz of 2.70 GHz")));
    assert!(shows(&mut app, text("Your processes only")));
    assert!(!shows(&mut app, text("Accelerators")), "no NPU, no section");
    assert!(shows(&mut app, text("Firefox")));
}

#[test]
fn the_selector_switches_to_the_sleeping_discrete_gpu() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    assert!(!shows(&mut app, text(super::header::ASLEEP_NOTE)));
    app.click(role(Role::RadioButton).name("GeForce RTX 4050"));
    app.settle();
    assert!(shows(&mut app, text(super::header::ASLEEP_NOTE)));
    app.click(role(Role::RadioButton).name("Radeon 780M"));
    app.settle();
    assert!(!shows(&mut app, text(super::header::ASLEEP_NOTE)));
}

#[test]
fn a_machine_without_gpus_explains_itself() {
    let mut app = mount(Default::default(), super::page);
    app.settle();
    assert!(shows(
        &mut app,
        text("No GPU was found under /sys/class/drm.")
    ));
}

#[test]
fn accelerators_appear_only_when_an_npu_exists() {
    let mut tick = seeded_tick(Swap::Configured);
    tick.gpu.accelerators = vec![AcceleratorSnapshot {
        key: DeviceKey::new("accel0"),
        name: "Ryzen AI NPU".into(),
        driver: "amdxdna".into(),
        power_state: GpuPower::Active,
        busy: Some(Percent::new(12.0)),
    }];
    let mut app = mount(live_from(tick), super::page);
    app.settle();
    assert!(shows(&mut app, text("Accelerators")));
    assert!(shows(&mut app, text("amdxdna · 12% busy")));
}

#[test]
fn utilization_is_always_live_over_fifteen_minutes() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    for toggle in ["Live", "History", "5 min", "15 min"] {
        assert!(
            !shows(&mut app, role(Role::RadioButton).name(toggle)),
            "the {toggle} toggle is still shown"
        );
    }
}
