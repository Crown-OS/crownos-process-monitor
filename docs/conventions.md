# Engineering conventions

Every contributor (human or agent) follows these rules. The feature reference is
`docs/implementation-plan.md`; this file says how code is written and who owns what.

## Rules

1. Meaningful, self-explanatory names for variables, functions, traits and modules.
2. Idiomatic Rust, edition 2024, toolchain 1.97 (pinned in `rust-toolchain.toml`).
3. Keep it simple: the best performance with the fewest lines. Focus on low CPU,
   low memory and no needless allocation. Use hardware acceleration where it exists.
4. No useless comments. If code needs a comment to be understood, restructure it.
   Doc comments on public items are welcome when they say something the
   signature does not (units, invariants, privileges, costs).
5. Don't bloat one file: split into focused modules and files.
6. Prefer borrowing over cloning. Use `Cow<str>` when ownership is conditional.
   Minimize lifetime annotations. Return owned data from constructors.
7. Avoid `unwrap`/`expect`. They are lint warnings in the workspace and gates run
   with `-D warnings`. Use `?`, `let else`, `ok_or`, `unwrap_or`.
8. Library crates (`crates/*`) use `thiserror`. The app (`src/`) uses `anyhow`.
9. Utility functions live in `src/util/<name>.rs` of the crate that owns them.
10. Make invalid states unrepresentable: newtypes (`Pid`, `Bytes`, `Percent`…), enums
    instead of booleans for modes, `#[must_use]` on important results.
11. Iterators over manual loops where clearer. `Vec::with_capacity` for known sizes.
    Named constants, not magic numbers. Early returns, not deep nesting.
12. **No subprocesses, ever** (no `nvidia-smi`, `smartctl`, `ip`, `ss`, `lsblk`…).
13. No blocking in async code. The sampler is a plain thread; D-Bus runs on
    background threads or tasks and never stalls a sample.
14. `unsafe` is denied workspace-wide. If a syscall truly needs it, use a rustix or
    a crate API instead.
15. Unit tests sit in the same file (`#[cfg(test)] mod tests`). Parser tests use
    inline fixture strings copied from real `/proc` and `/sys` files.
16. Never commit. The user commits.

## Quality gates (each crate, before reporting done)

```
cargo fmt -p <crate> --check
cargo clippy -p <crate> --all-targets -- -D warnings
cargo test -p <crate>
```

Use a private target directory so parallel builds don't block each other:
`CARGO_TARGET_DIR=target/agent-<crate>`. The UI crate uses the default `target/`.

## Workspace and ownership

| Crate | Owns | Depends on |
|---|---|---|
| `monitor-core` | ids, units, clocks, ring buffer, collector trait, interest, fd index, `/proc` and `/sys` readers | — |
| `monitor-system` | CPU, memory, process table, app grouping, inspector details, process actions | core |
| `monitor-io` | disks, volumes, SMART, network interfaces, sockets, Wi-Fi | core |
| `monitor-hardware` | GPUs (AMD, NVIDIA, Intel, generic DRM), NPUs, hwmon sensors and fans, battery and power, sleep inhibitors | core |
| `monitor-history` | rollups, SQLite store, sessions and gaps, range queries, CSV export | core |
| `monitor-alerts` | alert rules, detection, Worth a Look feed model | core |
| `monitor-engine` | sampler thread, scheduling by interest, snapshot composition, per-app joins, live rings | all of the above |
| `monitor-helper` | optional privileged D-Bus helper, modular `Authorizer` | core |
| app (`src/`) | CrownUI shell, components, pages, inspector drawer, settings dialog | engine, core, crownui |

Rules for shared code:
- Only edit your own crate.
- Never edit `monitor-core`, the root `Cargo.toml` or another crate. If you need
  something in core, implement it locally and list the proposal in your report.
- New external dependencies go in your crate's `Cargo.toml`. Add them to
  `[workspace.dependencies]` only if they are already there, otherwise use a plain
  version in your crate.

## `monitor-core` API (stable)

- `Pid`, `ProcessKey { pid, start_ticks }`, `Uid`, `DeviceKey(Arc<str>)`
- `AppKey::new(AppKind, id)`, `AppKey::parse`, `.kind()`, `.id()`.
  `AppKind::{Desktop, Flatpak, Snap, Terminal, Container, Service, Kernel, OtherUser, Process}`
- Units: `Bytes`, `BytesPerSecond::between(earlier, later, seconds)` (handles counter
  resets), `Percent::{new, clamped, from_ratio}`, `Celsius::from_millidegrees`,
  `Watts::{from_microwatts, from_milliwatts}`, `Rpm`, `Megahertz::{from_khz, from_hz}`,
  `Coverage::{Full, OwnProcesses, TcpOnly, Estimated, Unavailable}`
- `Tick::now()`, `Tick::since(&earlier) -> Elapsed { seconds, suspended }`.
  When `suspended` is set, record a gap instead of a rate.
- `RingSeries<T>` (fixed capacity, `push`, `iter`, `recent`, `as_slices`,
  `push_gap`/`max_recent`/`mean_recent` for `f32`)
- Collector trait: `Collector { type Output: Default + Clone; fn sample(&mut self, &SampleContext, &mut Output) -> SourceResult<()> }`.
  The output lives across ticks, so clear and refill vectors instead of reallocating.
- `SampleContext { tick, elapsed, interest: &Interest, fds: &FdIndex }`
- `Interest { page, window, inspected, expanded_app, selected_gpu, visible_interval }`:
  - `.shows(Page::Gpu)` is true when that page is on screen; skip expensive work when it's false.
  - `.sample_interval()` is 1 s visible, 5 s minimized.
- `FdIndex`: `socket_owner(inode)` and `drm_fds()`. The engine refreshes it for the
  sockets and DRM scope only when needed.
- `SourceError::{io, parse, Unavailable}`, `.is_gone()`, `.is_permission_denied()`,
  `SourceResult<T>`
- `util::parse`: `parse_u64/u32/i64/f64/hex_u32`, `lines`, `fields`, `key_value`,
  `kib_value`, `find_value`, `as_str`
- `util::sysfs`:
  - `PersistentFile::{open, open_at, read(&mut Vec<u8>), read_u64, read_i64}`: keep hot files open and `pread` them.
  - `read_into`, `read_at`, `read_small`, `read_u64`, `read_i64`, `read_string`, `link_name`
- `util::procfs`: `ProcRoot::{open, list_pids(&mut Vec<Pid>), open_pid(pid), as_fd}`, `PidName`
- `util::cadence`: `Cadence::{due, reset}` and `interval_elapsed(last, now, interval)` for
  work that runs at most once per interval of monotonic time.
- `util::latest`: `Latest<T>::{publish, try_get}`, the newest value a background worker
  published; readers `try_lock` and never stall a tick.
- `util::runtime_thread`: `spawn_runtime_thread(name, task)` runs a future on its own thread
  with a current-thread tokio runtime (D-Bus workers).
- `util::deferred`: `DeferredReader` for blocking reads moved off the sampler thread.
- `util::time`: `wall_unix_seconds`, `unix_now`, `MINUTE/FIVE_MINUTES/HOUR/DAY/WEEK_SECONDS`,
  `align_down`, `align_down_from`, `local_day`, `local_midnight`.

## Reference hardware (this machine)

- **CPU:** Ryzen 7 7840HS, 16 threads, `amd-pstate-epp`
- **Memory:** 15 GB RAM, zram swap
- **GPUs:** AMD 780M iGPU (amdgpu, card1, `0000:06:00.0`) plus NVIDIA RTX 4050 Laptop dGPU (open
  kernel module 615.78, card0, `0000:01:00.0`, NVML at `/usr/lib/libnvidia-ml.so.1`)
- **Storage:** 2× NVMe
- **Network:** `wlo1` Wi-Fi via NetworkManager, `tailscale0`, `waydroid0`, a veth
- **hwmon:** k10temp, amdgpu, nvme×2, legion_hwmon (2 fans), yogafan, BAT1, acpitz
- **Kernel:** cgroup v2, PSI on, kernel 7.2.
- **Not readable here:** RAPL `energy_uj` is root-only, and `io.stat` is not delegated to user scopes.
