/// ISA extensions worth showing, with the `/proc/cpuinfo` flag that names
/// them on any architecture.
const FLAG_HIGHLIGHTS: &[(&str, &str)] = &[
    ("sse4_2", "SSE4.2"),
    ("avx", "AVX"),
    ("avx2", "AVX2"),
    ("fma", "FMA"),
    ("avx512f", "AVX-512F"),
    ("avx512_vnni", "AVX-512 VNNI"),
    ("avx512_bf16", "AVX-512 BF16"),
    ("avx512_fp16", "AVX-512 FP16"),
    ("avx_vnni", "AVX-VNNI"),
    ("amx_tile", "AMX"),
    ("aes", "AES"),
    ("vaes", "VAES"),
    ("sha_ni", "SHA"),
    ("gfni", "GFNI"),
    ("bmi2", "BMI2"),
    ("rdrand", "RDRAND"),
    ("rdseed", "RDSEED"),
    ("asimd", "NEON"),
    ("sve", "SVE"),
    ("sve2", "SVE2"),
    ("sha2", "SHA2"),
    ("sha3", "SHA3"),
    ("crc32", "CRC32"),
    ("atomics", "LSE atomics"),
    ("i8mm", "I8MM"),
    ("bf16", "BF16"),
];

pub(crate) fn highlights_from_flags(flags: &str) -> Vec<&'static str> {
    let present: Vec<&str> = flags.split_whitespace().collect();
    FLAG_HIGHLIGHTS
        .iter()
        .filter(|(flag, _)| present.contains(flag))
        .map(|&(_, name)| name)
        .collect()
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) mod x86 {
    use raw_cpuid::CpuId;

    const HYBRID_LEAF: u32 = 7;
    const HYBRID_BIT: u32 = 15;

    pub(crate) struct X86Features {
        pub highlights: Vec<&'static str>,
        pub virtualization: Option<&'static str>,
        pub hypervisor: bool,
        pub hybrid: bool,
    }

    pub(crate) fn detect() -> X86Features {
        let cpuid = CpuId::new();
        let basic = cpuid.get_feature_info();
        let extended = cpuid.get_extended_feature_info();
        let amd = cpuid.get_extended_processor_and_feature_identifiers();
        let basic_flag =
            |check: fn(&raw_cpuid::FeatureInfo) -> bool| basic.as_ref().is_some_and(check);
        let extended_flag =
            |check: fn(&raw_cpuid::ExtendedFeatures) -> bool| extended.as_ref().is_some_and(check);
        let candidates = [
            (basic_flag(|info| info.has_sse42()), "SSE4.2"),
            (basic_flag(|info| info.has_avx()), "AVX"),
            (extended_flag(|info| info.has_avx2()), "AVX2"),
            (basic_flag(|info| info.has_fma()), "FMA"),
            (extended_flag(|info| info.has_avx512f()), "AVX-512F"),
            (extended_flag(|info| info.has_avx512vnni()), "AVX-512 VNNI"),
            (extended_flag(|info| info.has_avx512_bf16()), "AVX-512 BF16"),
            (extended_flag(|info| info.has_avx512_fp16()), "AVX-512 FP16"),
            (extended_flag(|info| info.has_avx_vnni()), "AVX-VNNI"),
            (extended_flag(|info| info.has_amx_tile()), "AMX"),
            (basic_flag(|info| info.has_aesni()), "AES"),
            (extended_flag(|info| info.has_vaes()), "VAES"),
            (extended_flag(|info| info.has_sha()), "SHA"),
            (extended_flag(|info| info.has_gfni()), "GFNI"),
            (extended_flag(|info| info.has_bmi2()), "BMI2"),
            (basic_flag(|info| info.has_rdrand()), "RDRAND"),
            (extended_flag(|info| info.has_rdseed()), "RDSEED"),
        ];
        let virtualization = if basic_flag(|info| info.has_vmx()) {
            Some("Intel VT-x")
        } else if amd.as_ref().is_some_and(|info| info.has_svm()) {
            Some("AMD-V")
        } else {
            None
        };
        let max_leaf = raw_cpuid::cpuid!(0).eax;
        X86Features {
            highlights: candidates
                .into_iter()
                .filter_map(|(present, name)| present.then_some(name))
                .collect(),
            virtualization,
            hypervisor: basic_flag(|info| info.has_hypervisor()),
            hybrid: max_leaf >= HYBRID_LEAF
                && raw_cpuid::cpuid!(HYBRID_LEAF, 0).edx & (1 << HYBRID_BIT) != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_highlights_from_flags() {
        let flags = "fpu vme sse4_2 avx avx2 aes sha_ni avx512f avx512_vnni";
        assert_eq!(
            highlights_from_flags(flags),
            [
                "SSE4.2",
                "AVX",
                "AVX2",
                "AVX-512F",
                "AVX-512 VNNI",
                "AES",
                "SHA"
            ]
        );
        assert_eq!(highlights_from_flags("fp asimd sve"), ["NEON", "SVE"]);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn detects_baseline_x86_features() {
        let features = x86::detect();
        assert!(features.highlights.contains(&"SSE4.2"));
    }
}
