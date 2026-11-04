const MAGIC: &[u8; 4] = b"\x7fELF";
const CLASS_OFFSET: usize = 4;
const DATA_OFFSET: usize = 5;
const MACHINE_OFFSET: usize = 18;
pub(crate) const HEADER_PREFIX: usize = 20;
const LITTLE_ENDIAN: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfClass {
    Elf32,
    Elf64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElfInfo {
    pub class: ElfClass,
    /// `e_machine`.
    pub machine: u16,
}

impl ElfInfo {
    pub fn parse(header: &[u8]) -> Option<Self> {
        if header.len() < HEADER_PREFIX || !header.starts_with(MAGIC) {
            return None;
        }
        let class = match header[CLASS_OFFSET] {
            1 => ElfClass::Elf32,
            2 => ElfClass::Elf64,
            _ => return None,
        };
        let bytes = [header[MACHINE_OFFSET], header[MACHINE_OFFSET + 1]];
        let machine = if header[DATA_OFFSET] == LITTLE_ENDIAN {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        };
        Some(Self { class, machine })
    }

    /// The architecture name as `uname -m` spells it.
    pub fn architecture(&self) -> &'static str {
        match (self.machine, self.class) {
            (3, _) => "i386",
            (62, _) => "x86_64",
            (40, _) => "arm",
            (183, _) => "aarch64",
            (243, ElfClass::Elf64) => "riscv64",
            (243, ElfClass::Elf32) => "riscv32",
            (8, _) => "mips",
            (20, _) => "ppc",
            (21, _) => "ppc64",
            (22, _) => "s390x",
            (258, _) => "loongarch64",
            _ => "unknown",
        }
    }

    pub fn native_architecture() -> &'static str {
        std::env::consts::ARCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers() {
        let mut x86_64 = [0_u8; HEADER_PREFIX];
        x86_64[..4].copy_from_slice(MAGIC);
        x86_64[CLASS_OFFSET] = 2;
        x86_64[DATA_OFFSET] = 1;
        x86_64[MACHINE_OFFSET] = 62;
        let info = ElfInfo::parse(&x86_64);
        assert_eq!(info.map(|info| info.architecture()), Some("x86_64"));
        assert_eq!(info.map(|info| info.class), Some(ElfClass::Elf64));
        let mut i386 = x86_64;
        i386[CLASS_OFFSET] = 1;
        i386[MACHINE_OFFSET] = 3;
        assert_eq!(
            ElfInfo::parse(&i386).map(|info| info.architecture()),
            Some("i386")
        );
        assert!(ElfInfo::parse(b"#!/bin/sh\n").is_none());
    }

    #[test]
    fn reads_own_executable() {
        use std::io::Read;
        let mut header = [0_u8; HEADER_PREFIX];
        let read =
            std::fs::File::open("/proc/self/exe").and_then(|mut file| file.read_exact(&mut header));
        assert!(read.is_ok());
        let info = ElfInfo::parse(&header);
        assert_eq!(
            info.map(|info| info.architecture()),
            Some(ElfInfo::native_architecture())
        );
    }
}
