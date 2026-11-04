pub(super) const KITTY: &str = "[Desktop Entry]\n\
Version=1.0\n\
Type=Application\n\
Name=kitty\n\
GenericName=Terminal emulator\n\
Comment=Fast, feature-rich, GPU based terminal\n\
TryExec=kitty\n\
StartupNotify=true\n\
Exec=kitty\n\
Icon=kitty\n\
Categories=System;TerminalEmulator;\n";

pub(super) const ORCA: &str = "[Desktop Entry]\n\
Name=Orca\n\
Exec=stably-orca %U\n\
Icon=stably-orca\n\
StartupWMClass=orca\n\
Type=Application\n\
[Desktop Action new-window]\n\
Name=New Window\n\
Exec=stably-orca --new-window\n";

pub(super) const VM_LAUNCHER: &str = "[Desktop Entry]\nType=Application\nName=macOS Sonoma\n\
Exec=kitty /home/marvelxcodes/OSX-KVM/vms/sonoma/start-macos.sh\nIcon=computer\n";
pub(super) const URL_HANDLER: &str = "[Desktop Entry]\nType=Application\nName=Claude Code URL Handler\n\
Exec=\"/opt/claude-code/bin/claude\" --handle-uri %u\nNoDisplay=true\n";
