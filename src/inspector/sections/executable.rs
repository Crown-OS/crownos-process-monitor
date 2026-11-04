use crownui::prelude::AnyView;
use monitor_engine::system::inspect::{ElfInfo, Emulation, ProcessDetails};

use crate::inspector::fields::{Fields, field_section};

pub fn emulation_name(emulation: &Emulation) -> String {
    match emulation {
        Emulation::Wine => "Wine".to_owned(),
        Emulation::Proton => "Proton".to_owned(),
        Emulation::Box64 => "box64".to_owned(),
        Emulation::Fex => "FEX".to_owned(),
        Emulation::Binfmt {
            handler,
            interpreter,
        } => format!("{handler} ({interpreter})"),
    }
}

fn architecture(elf: Option<ElfInfo>) -> Option<String> {
    let elf = elf?;
    let architecture = elf.architecture();
    Some(if architecture == ElfInfo::native_architecture() {
        architecture.to_owned()
    } else {
        format!("{architecture} (not native)")
    })
}

pub fn view(details: &ProcessDetails) -> Option<AnyView> {
    let executable = &details.executable;
    let mut fields = Fields::default();
    fields
        .add_some("Path", executable.path.clone())
        .add(
            "Updated on disk",
            if executable.deleted {
                "Yes: restart it to run the new version"
            } else {
                ""
            },
        )
        .add_some("Architecture", architecture(executable.elf))
        .add_some(
            "Emulation",
            executable.emulation.as_ref().map(emulation_name),
        )
        .add("Command line", details.command_line.join(" "))
        .add_some("Working directory", details.working_directory.clone());
    field_section("Executable", fields)
}
