use crowntest::{Role, role, text};

use crate::app::live::fixture::{Swap, mount, seeded_live, shows};

#[test]
fn every_memory_section_renders_with_seeded_data() {
    let mut app = mount(seeded_live(Swap::Configured), super::page);
    app.settle();
    for section in ["Breakdown", "Swap", "Compression", "Activity", "Apps"] {
        assert!(shows(&mut app, text(section)), "{section} is missing");
    }
    for removed in ["Pressure", "Memory modules", "Growth over 24 h", "Samsung"] {
        assert!(!shows(&mut app, text(removed)), "{removed} is still shown");
    }
    for toggle in ["Live", "History", "5 min", "15 min"] {
        assert!(
            !shows(&mut app, role(Role::RadioButton).name(toggle)),
            "the {toggle} toggle is still shown"
        );
    }
    for category in [
        "Apps",
        "Shared",
        "Cache",
        "Kernel",
        "Locked",
        "Compressed",
        "Other",
        "Free",
    ] {
        assert!(shows(&mut app, text(category)), "{category} is missing");
    }
    assert!(shows(&mut app, text("1.00 GB → 256 MB · 4.0× · zstd")));
    assert!(shows(&mut app, text(super::table::METHOD_NOTE)));
    assert!(shows(&mut app, text("RSS")));
    assert!(shows(&mut app, text("PSS")));
}

#[test]
fn the_swap_chart_shows_only_with_swap() {
    let mut with_swap = mount(seeded_live(Swap::Configured), super::page);
    with_swap.settle();
    assert!(shows(
        &mut with_swap,
        text("zram · 1.00 GB of 8.00 GB · priority 100")
    ));
    assert!(!shows(&mut with_swap, text(super::swap::NO_SWAP)));

    let mut without = mount(seeded_live(Swap::None), super::page);
    without.settle();
    assert!(shows(&mut without, text(super::swap::NO_SWAP)));
    assert!(!shows(&mut without, text("Swap in · out")));
    assert!(shows(&mut without, text("No memory compression")));
}
