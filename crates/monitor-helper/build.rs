//! With the `ebpf` feature, compiles the `cgroup_skb` programs with clang
//! (override with `CLANG=/path/to/clang`) into `$OUT_DIR`.

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const PROGRAM_SOURCE: &str = "src/bpf/cgroup_traffic.bpf.c";
const PROGRAM_OBJECT: &str = "cgroup_traffic.bpf.o";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={PROGRAM_SOURCE}");
    println!("cargo::rerun-if-env-changed=CLANG");
    if env::var_os("CARGO_FEATURE_EBPF").is_none() {
        return Ok(());
    }
    let object =
        PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?).join(PROGRAM_OBJECT);
    let clang = env::var_os("CLANG").unwrap_or_else(|| "clang".into());
    let status = Command::new(&clang)
        .args([
            "-O2",
            "-Wall",
            "-Werror",
            "-target",
            "bpf",
            "-c",
            PROGRAM_SOURCE,
            "-o",
        ])
        .arg(&object)
        .status()
        .map_err(|error| {
            format!(
                "the `ebpf` feature needs clang with the BPF target ({}): {error}",
                clang.to_string_lossy()
            )
        })?;
    if !status.success() {
        return Err(format!("clang failed to compile {PROGRAM_SOURCE}: {status}").into());
    }
    Ok(())
}
