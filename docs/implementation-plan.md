# CrownOS Process Monitor: Implementation Blueprint

Status: research complete, decisions recorded (§11). No implementation code written yet.
Target: a Linux-native counterpart to Vitals for macOS (vitalsmac.com, v1.9.7), built only with **CrownUI**, with exactly six sidebar pages: **Overview, CPU, GPU, Memory, Disk, Network**.

---

## 0. Research summary

### 0.1 How the research was done

- **vitalsmac.com in Orca's built-in browser.** I drove the site's interactive demo tab by tab and extracted each panel's text, its DOM structure (chart SVGs, rings, controls) and its links.
- **Vitals' own docs.** I read `llms-full.txt` (312 KB: every feature page, changelogs 1.4–1.9, FAQ) and the `history-data` schema page.
- **CrownUI.** I surveyed `../crownui` and the sibling apps that use it, and spot-checked the key APIs (`canvas`, `DrawList`, `lazy_list`, `cx.spawn`, `spawn_worker`).
- **Linux interfaces, on real hardware.** I probed this machine, which is a good hybrid test rig:
  - CPU: Ryzen 7 7840HS, `amd-pstate-epp`
  - GPUs: **AMD 780M iGPU (amdgpu) + NVIDIA dGPU (open kernel module 615.78, NVML present)**
  - Storage and memory: 2× NVMe, zram swap
  - Sensors: `legion_hwmon` fans
  - Kernel and cgroups: cgroup v2, PSI enabled, kernel 7.2

### 0.2 What Vitals actually does (feature inventory)

**Window layout.** The window has 13 tabs: Overview, CPU, Memory, Disk, Network, GPU, Neural, Battery, Sound, Bluetooth, Projects, Agents, Containers. Most tabs share one layout:

1. **Hero stat strip.** The headline value plus 3–6 secondary figures.
2. **Area chart.** Gradient-filled area with a line on top, showing the last 5, 15 or 30 minutes.
3. **"Top App" callout.**
4. **App table.** One row per app, with its process count and the tab's metric. Rows expand into their processes.

**Overview.**
- Each subsystem has a card with the headline figure, 2–3 stats and a sparkline. Clicking a card opens its tab.
- Donut rings: *Memory by Type*, *Memory by App* (top 4 + Other) and *Power by App*.
- **Over Time** history chart with a range selector.
- **Peaks** card and **At a Limit** stretches.
- **Worth a Look**: the alert feed, grouped by day.

**Cross-cutting features.**
- **App grouping.** Helper processes are counted under the app that started them (about 60 rows instead of about 1000).
- **Browser split.** A browser's total splits into tabs, extensions, GPU process and the browser itself.
- **Process Inspector drawer.** Identity, launch info, security, lifetime usage, ports, children, environment, export as CSV or text.
- **Ended Recently.** Processes that ended in the last 10 minutes stay listed.
- **Table tools.** Pinning apps, ⌘F find, `/regex/` filter, a Columns menu, a right-click menu.
- **Process control.** Quit and Force Quit, always confirmed, with a count of the processes that will close.
- **Alerts.** Sustained CPU (10 min), memory growth (1 h), disk/net bursts, heat, and per-app custom limits, with a 1-hour cooldown.
- **History.** Up to 1 year in a documented SQLite file: 5-minute readings, minute/5-minute/hour system rollups, and app hours/days.
- **Units.** °C or °F, bytes or bits, CPU per core or per machine.
- **Themes.** Monochrome or colour-blind-safe palettes, compact layout.
- **Export.** CSV and image export.
- **Low overhead.** With no window open it samples every 5 s at about 0.1 % of one core, and samples faster only while visible. (Vitals keeps running in the menu bar; this app does not, see D3.)

### 0.3 Scope mapping onto our six pages

| Vitals feature | Where it goes here |
|---|---|
| Overview, CPU, Memory, Disk, Network, GPU tabs | Their own pages (1:1) |
| Temperatures and fans, fan control | CPU page (CPU sensors and fans) and GPU page (GPU sensors). Hottest sensor on Overview. |
| Neural Engine | GPU page → **Accelerators** section (AMD XDNA / Intel NPU via `/sys/class/accel`). Hardware-dependent. |
| Battery: power draw, power by app | Overview → Power card and *Power by App* ring. Sleep inhibitors: Overview detail. |
| Containers (Docker/Podman) | Not a page. A **grouping source**: containers appear as apps in every table (`docker-<id>.scope`, `libpod-<id>.scope`). |
| Projects / dev servers / ports | Not a page. Listening ports appear in the Network page and the Inspector. |
| Worth a Look, Peaks, Over Time | Overview sections |
| Sound, Bluetooth, Agents, AI limits, menu bar, MCP, CLI | **Out of scope.** No page may be added. Listed in §10 as possible future work outside the sidebar. |
| Settings | Header gear → settings **dialog** (not a sidebar page). Decided (D1). |

Legend used below:
- **[U]** universally available to an unprivileged user
- **[H]** hardware- or driver-dependent
- **[O]** only for the user's own processes without the optional helper
- **[P]** needs the optional privileged helper (§4.9)

---

## 1. Feature checklist

### 1.1 Shared, on every page

- [ ] Hero stat strip with live values [U]
- [ ] Live area chart with a 5, 15 or 30 minute window, hover crosshair and tooltip [U]
- [ ] **Live / History** switch. History ranges: 1 h, 12 h, 24 h, 7 d, 30 d, 90 d, 6 mo, 1 y. Beyond 90 days, one bar per week. [U]
- [ ] "Top App" callout for the page's metric [U]
- [ ] **App table**, virtualized:
  - one row per app, with icon, name, process count and the page's metric columns
  - expand a row into its processes
  - pinned apps stay on top
- [ ] Sort by any column, ascending or descending [U]
- [ ] Find (Ctrl+F): substring, or `/regex/` [U]
- [ ] Columns menu: PID, user, path, start time, threads, state, nice. Saved per page. [U]
- [ ] Filter chips: My apps / All / System / Containers. A "show system processes" switch. [U]
- [ ] Right-click menu:
  - copy name, path or command line
  - inspect, quit, force quit, pause/resume, renice
  - "Alert when…" and "Don't alert about…"
- [ ] Process Inspector drawer (§1.8) [U/O]
- [ ] Ended Recently group: processes gone within the last 10 minutes, with name, start/end time and peak memory [U]
- [ ] New mark on processes younger than 10 s [U]
- [ ] Empty, loading and "unavailable on this hardware" states [U]
- [ ] Sidebar items show trailing live values (`sidebar_item.trailing_text`) [U]
- [ ] Readings turn red under strain, using the same thresholds as alerts [U]

### 1.2 Overview

- [ ] **CPU card**: now %, user %, system %, average today, sparkline [U]
- [ ] **Memory card**: used of total, pressure level (Normal / Elevated / Critical), apps, kernel, compressed, sparkline [U]
- [ ] **GPU card**: per GPU (selector if more than one), name, utilization, VRAM used, average and peak. Asleep dGPU state. [H]
- [ ] **Disk card**: free of total (root fs), read/s, write/s, written today [U]
- [ ] **Network card**: down/s, up/s, today, last 7 days [U]
- [ ] **Power card**:
  - system power draw [H]
  - battery %, time remaining, health [H]
  - AC/battery state [H]
- [ ] **Sensors tile**: hottest sensor, CPU and GPU temperature, fan rpm [H]
- [ ] **Accelerator tile** if an NPU exists [H]
- [ ] **Memory by Type** ring [U]
- [ ] **Memory by App** ring: top 4 + Other [U]
- [ ] **CPU by App** ring [U]
- [ ] **Power by App** ring: estimated (§2.1). Hidden when no energy source exists. [H/P]
- [ ] **Over Time** chart with a metric tab bar (CPU, Memory, GPU, Disk, Network, Power, Temperature), a range menu and Export CSV [U]
- [ ] **Busiest apps by hour** for the last 48 h. Click an hour to see its ranking. [U]
- [ ] **Peaks** card: highest CPU, memory pressure, temperature, power and GPU in the range, each with its time [U/H]
- [ ] **At a Limit** stretches: CPU or GPU ≥ 90 %, memory pressure, temperature ≥ threshold, thermal throttling, each with start, end and duration [U/H]
- [ ] **Worth a Look** feed:
  - alerts grouped by day, repeats merged with a count
  - each row shows the current state
  - pin, narrow by type, quiet until tomorrow or always
- [ ] Uptime, process count, thread count, load average [U]
- [ ] What keeps the machine awake: logind inhibitors (who, why, mode, since) [U]
- [ ] Clicking a card navigates to its page [U]

### 1.3 CPU

- [ ] Total utilization now, average today, peak in window [U]
- [ ] State breakdown: user, nice, system, iowait, irq, softirq, steal, guest. Stacked area chart. [U]
- [ ] Per-core grid: one mini bar or sparkline per logical CPU, grouped by core type and physical core [U]
- [ ] Frequency: per-core current, average, min/max, boost state [H]
- [ ] Governor / EPP / scaling driver / platform power profile [H]
- [ ] Load average 1/5/15 vs logical CPU count [U]
- [ ] PSI CPU pressure (some avg10/60/300) [U] (kernel CONFIG_PSI)
- [ ] Context switches/s, interrupts/s, soft interrupts/s, forks/s, running/blocked tasks [U]
- [ ] Process and thread totals, uptime [U]
- [ ] **Hardware panel**:
  - vendor, model, microarchitecture, family/model/stepping
  - sockets, cores, threads, SMT state
  - hybrid layout (P/E cores: Intel `cpu_core`/`cpu_atom`, Arm capacity, AMD dense cores)
  - caches L1d/L1i/L2/L3, base and max clock
  - ISA flags (AVX2, AVX-512, …), virtualization support, vulnerabilities summary, microcode
- [ ] **Sensors and fans**:
  - package and per-CCD/core temperatures, chart [H]
  - fan rpm per fan [H]
  - throttle event counters (Intel) [H]
- [ ] **Fan control**: automatic or a fixed % within the device's range. Restored to automatic on exit. [H/P]
- [ ] CPU package power (RAPL / AMD energy) [H/P]
- [ ] App table: CPU % (per core or per machine, unit setting), CPU time, threads, nice [U]
- [ ] Per-process CPU affinity and priority actions [U/O]

### 1.4 GPU

- [ ] **GPU selector** when there is more than one. Each GPU shows: integrated/discrete badge, driver, PCI address, primary/boot VGA, connected displays. [U]
- [ ] **Runtime-PM state**: dGPU "Asleep" shown **without waking it** [U]
- [ ] Utilization total [H]
- [ ] Per-engine utilization: 3D/graphics, compute, copy/blit, video decode, video encode, video enhance [H]
- [ ] VRAM used/total, GTT/shared system memory used/total, visible VRAM [H]
- [ ] Clocks: core/shader and memory, current vs max. Performance level (P-state / DPM). [H]
- [ ] Temperatures: edge, junction/hotspot, memory [H]
- [ ] Power draw, power cap, voltage [H]
- [ ] Fan rpm/% [H]
- [ ] PCIe link: current/max generation and width. Throughput where available (NVIDIA). [H]
- [ ] Throttle / clock-event reasons (NVIDIA) [H]
- [ ] Average and peak for the window. Live chart, with engine series toggles. [H]
- [ ] **App table**: per-process GPU % (per engine on hover), VRAM and GTT per process, which GPU the process uses (hybrid offload visibility) [H/O]
- [ ] **Accelerators** section: NPU present, busy %, per-process users where exposed [H]
- [ ] Hardware panel: marketing name, vendor/device/subsystem IDs, VBIOS, driver version, compute units / SMs where exposed, VRAM type/width (NVIDIA) [H]

### 1.5 Memory

- [ ] Used of total (= Total − Available), available, free [U]
- [ ] Pressure level from PSI: Normal / Elevated / Critical. Some/full avg10, avg60. [U]
- [ ] **Breakdown ring and legend**: Apps (anon), Shared/tmpfs, Cache (reclaimable), Kernel (non-reclaimable), Compressed, Free, Other (remainder, so the ring always sums) [U]
- [ ] Charts:
  - memory used, stacked by category
  - pressure chart under it
  - swap used chart beside it once swap > 0
- [ ] Swap: per-device usage (partition, file, zram), swap in/out pages/s [U]
- [ ] **Compression**: zram original vs compressed, ratio, algorithm. zswap pool size and stored size. [H]
- [ ] Page faults/s (minor, major), OOM kills since boot. OOM kill event list via kernel log is out of scope (needs root). [U]
- [ ] Dirty and writeback, huge pages / THP usage, mlocked/unevictable [U]
- [ ] **Hardware panel**: DIMMs (slot, size, type, speed, form factor, manufacturer, part) from udev `MEMORY_DEVICE_*` [H]
- [ ] App table:
  - memory per app: cgroup-exact or PSS-based, with a method indicator
  - per process: RSS, PSS, USS, swap, peak (VmHWM)
  - memory growth sparkline
  [U/O]
- [ ] Per-app memory limit alert ("Alert when > 8 GB for 5 min") [U]

### 1.6 Disk

- [ ] **Volumes list**:
  - mount point, label, filesystem, device, size, used, free, % bar
  - read-only flag, removable/eject
  - pseudo/system mounts hidden
  - btrfs subvolume dedupe
  [U]
- [ ] Throughput read/write per second, total and per physical device [U]
- [ ] IOPS, average latency (r/w), utilization/busy %, queue depth (in flight), discards/s [U]
- [ ] PSI IO pressure [U]
- [ ] Read and written today, last 7 days, last 30 days, from history [U]
- [ ] **Drive panel** per physical device:
  - model, serial, firmware, capacity, NVMe/SATA/USB/virtual, SSD/HDD
  - scheduler, logical/physical sector size
  [U]
- [ ] Drive temperature (NVMe hwmon, `drivetemp` for SATA) [H]
- [ ] **Drive health**: SMART status and attributes. NVMe: percentage used, spare, media errors, power-on hours, data units written. [H] (UDisks2)
- [ ] Device stacking: LUKS/LVM/MD shown under the physical disk, with no double counting [U]
- [ ] App table: read/s, write/s, total read/written since start [O/P]

### 1.7 Network

- [ ] Download/upload per second, total and per interface [U]
- [ ] Today / last 7 days / last 30 days totals, per interface and total [U]
- [ ] **Interfaces list**:
  - name, kind (Wi-Fi, Ethernet, VPN/tun/WireGuard, bridge, veth/container, loopback), state
  - MAC, MTU, speed/duplex, IPv4/IPv6 addresses
  - packets, errors, drops
  [U]
- [ ] Default route / gateway, DNS servers [U]
- [ ] **Wi-Fi panel**: SSID, BSSID, signal dBm/quality, link rate, band/frequency/channel, security [H]
- [ ] Charts:
  - down/up mirrored area chart, per interface toggles
  - packets/s and errors on hover
  [U]
- [ ] **Connections**: per process, protocol, local and remote address:port, state, queue sizes [U] (owner mapping [O/P])
- [ ] **Listening ports**:
  - port, protocol, bind scope (loopback-only vs all interfaces), owning process/app
  - links to the inspector
  [U] (owner [O/P])
- [ ] App table: download/s, upload/s, totals since start. Coverage badge: "TCP only" without the helper; all protocols with it. [O/P]
- [ ] Public IP is **not** shown by default: it needs a network request (privacy) [U]

### 1.8 Process Inspector (drawer, opened from any table)

The macOS concepts don't exist on Linux, so each section maps to the nearest real Linux equivalent:

| Section | Linux content |
|---|---|
| Identity | PID, parent (link), app it belongs to, process kind (browser tab/extension/GPU/utility, Electron helper, kernel thread, container), user/UID/GID, state (R/S/D/Z/T/I), start time, running for, tty, session/pgid |
| Executable | Path, `(deleted)` flag ("updated on disk, restart to apply"), ELF arch (x86_64/aarch64/i386), emulation (Wine/Proton, FEX/box64/qemu-user via binfmt), command line, cwd |
| Security (stands in for code signature, sandbox and entitlements) | Sandbox type (flatpak/snap/bubblewrap/firejail/container, with the Flatpak app id or Snap name when sandboxed), namespaces that differ from init, seccomp mode, NoNewPrivs, effective/permitted/bounding capabilities decoded, LSM label (AppArmor/SELinux) |
| Startup (stands in for launch agent) | systemd unit (from cgroup) with description and unit file path via systemd D-Bus; autostart `.desktop` file if it matches |
| App details (stands in for Info.plist) | `.desktop` entry: name, id, icon, categories, exec, StartupWMClass. Whole `.desktop` file foldable. |
| Resource usage | CPU time user/system (+ children), memory RSS/PSS/USS/swap and peak VmHWM as % of peak, threads, open fds, major/minor faults, I/O bytes read/written, voluntary/involuntary context switches, nice/priority, scheduler policy, CPU affinity, OOM score/adj |
| GPU | Per-GPU engine time and memory from fdinfo/NVML |
| Network | Sockets and listening ports |
| Children | Process subtree, each a link |
| Environment | Folded, own processes only, never logged or exported unless asked |
| Live charts | CPU and memory of this process while the drawer is open |
| Export | Copy as text, save as CSV |

---

## 2. Metric and visualization breakdown

Rates are always `Δcounter / Δt_monotonic`. Percentages are 0–100, and "CPU of the machine" is normalised by the logical CPU count.

### 2.1 Overview

| Metric | Definition | Visualization |
|---|---|---|
| CPU now | from `/proc/stat` deltas (see CPU) | big number + area sparkline |
| CPU average today | running mean since local midnight (history) | stat |
| Memory used | `MemTotal − MemAvailable` | big number + sparkline |
| Pressure level | PSI memory: Normal if `some.avg10 < 10`, Elevated if `< 40` or `full.avg10 ≥ 5`, otherwise Critical. Named constants. Fallback: available ratio. | coloured label |
| Memory by Type | categories from §2.4 | ring (5–7 segments) + legend |
| Memory by App | top 4 apps by app memory + Other | ring + legend |
| Power draw | battery `power_now` (or `current_now × voltage_now`) on battery; RAPL package + GPU power on AC. Shown as "unknown" otherwise. | big number |
| Power by App | estimate per app: `Σ CPU energy × app CPU share + Σ GPU energy × app GPU share`. Labelled "estimated". | ring |
| Over Time | history series for the chosen metric and range | area chart (≤ 90 d) or weekly bars (> 90 d) |
| Peaks | max of `*_peak` columns in range, with timestamp | list |
| At a Limit | runs of consecutive 5-minute readings crossing a limit, merged | timeline list |
| Busiest by hour | `app_hours` ranked per hour | hour strip + ranked list |

### 2.2 CPU

| Metric | Source / formula | Cadence | Tier |
|---|---|---|---|
| Total % | `/proc/stat` cpu line: `busy = user+nice+system+irq+softirq+steal` (guest is already inside user; don't double count). `% = Δbusy / Δ(busy+idle+iowait)` | 1 s visible, 5 s minimized | U |
| Per-core % | `/proc/stat` cpuN lines | 1 s (CPU page only) | U |
| States | per-field deltas | 1 s | U |
| Frequency | `cpufreq/cpuinfo_avg_freq` (new kernels, cheap) → `scaling_cur_freq` fallback. `cpuinfo_max_freq`, `boost`. | 2 s, CPU page only | H |
| Governor/EPP/driver | `cpufreq/scaling_governor`, `energy_performance_preference`, `scaling_driver`. Power profile via `org.freedesktop.UPower.PowerProfiles`. | on change / 10 s | H |
| Load avg | `/proc/loadavg` | 5 s | U |
| PSI | `/proc/pressure/cpu` | 2 s | U |
| ctxt/intr/softirq/forks/running/blocked | `/proc/stat` `ctxt`, `intr` (first field), `softirq`, `processes`, `procs_running`, `procs_blocked` | 1 s | U |
| Topology | `/sys/devices/system/cpu/cpuN/topology/{core_id,die_id,cluster_id,package_id}`, `/sys/devices/cpu_core/cpus` + `cpu_atom/cpus` (Intel hybrid), `cpu_capacity` (Arm), CPUID core-type leaf (Intel 0x1A, AMD 0x80000026) | once + CPU hotplug | U/H |
| Caches | `/sys/devices/system/cpu/cpu0/cache/index*/{level,type,size,shared_cpu_list}` | once | U |
| Model/flags | `raw-cpuid` on x86; `/proc/cpuinfo` on Arm (MIDR → name table) | once | U |
| Vulnerabilities | `/sys/devices/system/cpu/vulnerabilities/*` | once | U |
| Microcode | `/proc/cpuinfo` `microcode` | once | U |
| Temperatures | hwmon by driver: `k10temp` (Tctl/Tccd*), `zenpower`, `coretemp` (Package id N / Core N), `thinkpad`, `legion_hwmon`, `dell_smm`, `asus-ec-sensors`, `nct67xx`/`it87` (desktop boards), `cpu_thermal` (Arm), `acpitz` last resort | 2 s | H |
| Fans | hwmon `fanN_input` (+ `fanN_label`), `pwmN` / `pwmN_enable` for control | 2 s | H / P to set |
| Throttling | Intel: `/sys/devices/system/cpu/cpuN/thermal_throttle/{core,package}_throttle_count`; otherwise inferred (frequency ≪ max while temperature ≥ Tjmax−5) | 5 s | H |
| Package power | `/sys/class/powercap/intel-rapl:0/energy_uj` (also AMD Zen via the `intel-rapl` name). **root-only since CVE-2020-8694** (verified 0400 here). | 2 s | P |
| Per-process CPU % | `/proc/<pid>/stat` fields 14–15 (`utime`, `stime`) delta ÷ (`CLK_TCK` × Δt). Per core: ×100; per machine: ÷ ncpu. Readable for **all** processes. | 1 s visible, 5 s minimized | U |
| Per-app CPU % | Σ process CPU %, **or** cgroup `cpu.stat usage_usec` delta when the app owns its scope (exact, includes processes that already exited) | same | U |
| Threads, nice | `/proc/<pid>/stat` fields 18–20 | same | U |

Visuals:
- headline area chart
- stacked-area state chart
- per-core grid of mini bars, coloured by load and grouped P/E
- frequency line chart
- temperature line chart with Tjmax/critical reference line
- fan rpm bars

### 2.3 GPU

GPUs are discovered by enumerating `/sys/class/drm/card[0-9]*` (skipping connector entries like `card0-DP-1`) → `device/` → `vendor`, `device`, `subsystem_*`, `driver` link, `boot_vga`, PCI BDF (`readlink`).
- Names come from `/usr/share/hwdata/pci.ids` (parsed lazily, only the present IDs) or the driver (NVML name, `/usr/share/libdrm/amdgpu.ids` marketing names).
- Integrated vs discrete: amdgpu APU flag, PCI bus 0 on Intel, `boot_vga`, and whether `mem_info_vram_total` is small.
- Displays: `/sys/class/drm/cardN-*/status == connected`.

**Wake guard (critical):** before *any* device query, read `/sys/bus/pci/devices/<bdf>/power/runtime_status`. If it is `suspended`, show "Asleep". Skip every sysfs read that would resume it, skip NVML, and use fdinfo only (reading fdinfo doesn't wake the device). Keep NVML device handles only while the GPU is active.

| Metric | AMD (amdgpu) | NVIDIA (proprietary/open) | Intel i915 | Intel xe | Generic DRM (panfrost, panthor, msm, v3d, etnaviv…) |
|---|---|---|---|---|---|
| Total util | `device/gpu_busy_percent`; `gpu_metrics` `average_gfx_activity` | NVML `nvmlDeviceGetUtilizationRates` | `1 − Δrc6_residency_ms/Δt` (`gt/gt0/rc6_residency_ms`); exact via i915 PMU (`perf_event_open`, needs CAP_PERFMON) [P] | `1 − Δgtidle/idle_residency_ms/Δt` (`device/tile0/gt0/gtidle/`) | Σ fdinfo engine deltas; devfreq `load` |
| Per-engine | fdinfo `drm-engine-{gfx,compute,dma,dec,enc,enc_1,jpeg}` (ns) | NVML encoder/decoder utilization | fdinfo `drm-engine-{render,copy,video,video-enhance,compute}` ns + `drm-engine-capacity-*` | fdinfo `drm-cycles-{rcs,bcs,vcs,vecs,ccs}` / `drm-total-cycles-*` | fdinfo `drm-engine-*` |
| VRAM | `mem_info_vram_{used,total}`, `mem_info_vis_vram_*`, `mem_info_gtt_*` | `nvmlDeviceGetMemoryInfo_v2` (used/reserved/total) | system memory: fdinfo `drm-total-system0` | `drm-total-vram0`, `drm-total-gtt` | fdinfo `drm-total-*` |
| Clocks | `pp_dpm_sclk`, `pp_dpm_mclk` (line with `*`), `gpu_metrics` | `nvmlDeviceGetClockInfo` / max | `gt_act_freq_mhz`, `gt_max_freq_mhz` | `tile0/gt0/freq0/{act,max}_freq` | devfreq `cur_freq` |
| Temp | hwmon `edge`, `junction`, `mem` | `nvmlDeviceGetTemperature` | hwmon (dGPU) / package temperature (iGPU) | hwmon (dGPU) | thermal zone |
| Power | hwmon `power1_average` or `power1_input`, `power1_cap` | `nvmlDeviceGetPowerUsage`, enforced limit | dGPU hwmon `energy1_input`; iGPU RAPL `uncore` [P] | hwmon `energy*_input` | — |
| Fan | hwmon `fan1_input`, `pwm1` | `nvmlDeviceGetFanSpeed_v2` | dGPU hwmon | dGPU hwmon | — |
| PCIe | `current_link_{speed,width}`, `max_link_*` (PCI sysfs, all vendors) | + `nvmlDeviceGetPcieThroughput` (slow, ~20 ms: every 5 s only) | sysfs | sysfs | — |
| Perf state / throttle | `power_dpm_force_performance_level` | `nvmlDeviceGetPerformanceState`, `GetCurrentClocksEventReasons` | — | `throttle/` reasons | — |
| Per-process util | fdinfo engine ns deltas [O/P] | `nvmlDeviceGetProcessUtilization(lastSeenTimeStamp)` (SM/enc/dec per pid, all users) | fdinfo [O/P] | fdinfo cycles [O/P] | fdinfo [O/P] |
| Per-process memory | fdinfo `drm-resident-vram`, `drm-total-gtt` | `nvmlDeviceGetComputeRunningProcesses_v3` + `GetGraphicsRunningProcesses_v3` (`usedGpuMemory`) | fdinfo | fdinfo | fdinfo |

fdinfo rules (from the kernel's `Documentation/gpu/drm-usage-stats.rst`):
- Scan `/proc/<pid>/fdinfo/<fd>` only for fds whose `readlink /proc/<pid>/fd/<fd>` points at `/dev/dri/*`.
- Dedupe by `(drm-pdev, drm-client-id)`, because the same client is shared across dup'd fds and forks.
- Map `drm-pdev` to a GPU. This is how hybrid offload shows which app runs on the dGPU.
- Clamp per-engine utilization to 100 %.
- Verified on this machine: amdgpu exposes `drm-engine-gfx`/`compute` and the memory keys. **nvidia-drm exposes no fdinfo stats**, so NVML is mandatory for NVIDIA.

nouveau gets limited support: hwmon temperature/power plus fdinfo memory if present, labelled "limited by driver".

Accelerators: `/sys/class/accel/accel*/device` (`amdxdna`, `intel_vpu`).
- Intel NPU busy: `npu_busy_time_us` delta.
- AMD XDNA: fdinfo `drm-engine-npu*` where the driver provides it.
- Not present on this machine (no driver loaded), so this stays a hardware-dependent section.

Visuals:
- GPU selector as a segmented control
- utilization area chart with per-engine overlay lines
- VRAM/GTT stacked bar
- clock, temperature and power line charts
- engine bars
- app table

### 2.4 Memory

| Category | `/proc/meminfo` formula |
|---|---|
| Used | `MemTotal − MemAvailable` |
| Apps (anonymous) | `AnonPages` |
| Shared / tmpfs | `Shmem` |
| Cache (reclaimable) | `Cached − Shmem + Buffers + SReclaimable` |
| Kernel | `SUnreclaim + KernelStack + PageTables + SecPageTables + Percpu` (+ `VmallocUsed` where meaningful) |
| Locked | `Mlocked`, `Unevictable` (detail) |
| Compressed | zram: `/sys/block/zramN/mm_stat` fields `orig_data_size`, `compr_data_size`, `mem_used_total`; zswap: `Zswap`, `Zswapped` |
| Free | `MemFree` |
| Other | `MemTotal − Σ shown` (keeps the ring consistent) |

Other memory sources:
- swap: `/proc/swaps`
- swap rates: `/proc/vmstat` `pswpin`/`pswpout`
- faults: `pgfault`/`pgmajfault`
- OOM kills: `oom_kill`
- PSI: `/proc/pressure/memory`
- THP: `AnonHugePages`
- hugepages: `HugePages_*`

**Per-process memory:**
- RSS: `/proc/<pid>/statm` (cheap, all processes) [U]
- PSS/USS/swap: `/proc/<pid>/smaps_rollup`. Own processes [O]; others need [P]. Expensive because the kernel walks page tables, so it is refreshed **round-robin**, about 50 processes per tick, plus immediately for the inspected process.
- Peak: `VmHWM` in `/proc/<pid>/status`.

**Per-app memory** uses the best method available, and the table shows which:
1. cgroup `memory.current` / `memory.stat` (`anon` + `file` + `kernel`) when the app owns a scope. It is exact, cheap and readable for own scopes. Verified here: `memory.current`, `memory.peak`, `memory.stat` are present in user scopes.
2. Σ PSS of member processes.
3. Σ RSS, marked "approximate (shared pages counted twice)".

DIMMs: udev properties of `/sys/devices/virtual/dmi/id` (`MEMORY_DEVICE_<n>_{SIZE,TYPE,SPEED_MTS,FORM_FACTOR,MANUFACTURER,PART_NUMBER,LOCATOR}`). These are readable unprivileged from the udev database (verified). Raw SMBIOS is root-only.

Visuals:
- used area chart stacked by category
- PSI line chart under it
- swap chart beside it, shown only when swap > 0
- category ring
- zram ratio gauge
- DIMM table

### 2.5 Disk

| Metric | Source |
|---|---|
| Volumes | `/proc/self/mountinfo`. Reuse the classification logic from `crownos-files/src/fs/mounts.rs`: pseudo, system and remote. Watched with `poll(POLLPRI)` on the file, so there's no polling. |
| Size/used/free | `statvfs` per real mount, deduped by `st_dev` / btrfs fsid (btrfs `statvfs` is approximate: label it) |
| Throughput, IOPS, latency, util, queue | `/proc/diskstats` (fields: reads, sectors read, ms reading, writes, sectors written, ms writing, in-flight, io_ticks, weighted ms, discards…). Sectors are always 512 B. Rate = Δsectors × 512 / Δt; util = Δio_ticks / Δt; avg latency = Δms / Δops. |
| Physical vs logical | Count only whole disks (`/sys/block/*` without `partition`, excluding `loop`, `ram`, `zram`, `dm-*`). Build the stack from `/sys/block/*/holders` and `slaves` so LUKS/LVM/MD appear under their disk. |
| Device info | `/sys/block/X/device/{model,serial,firmware_rev}`, `queue/{rotational,logical_block_size,physical_block_size,scheduler}`, `removable`, transport from the `subsystem` link (nvme/scsi/usb/virtio/mmc) |
| Temperature | NVMe hwmon (verified: `nvme` hwmon0/hwmon3); SATA via the `drivetemp` hwmon module |
| Health / SMART | UDisks2 D-Bus: `org.freedesktop.UDisks2.Drive.Ata` (`SmartGetAttributes`, `SmartFailing`, `SmartTemperature`, `SmartPowerOnSeconds`); `org.freedesktop.UDisks2.NVMe.Controller` (`SmartGetAttributes`: percentage used, available spare, media errors, data units read/written). No `smartctl` subprocess. |
| PSI | `/proc/pressure/io` |
| Per-process I/O | `/proc/<pid>/io` `read_bytes`, `write_bytes`, `cancelled_write_bytes` [O/P] |
| Per-app I/O | Σ process I/O. cgroup `io.stat` only where the io controller is delegated (**it is not delegated to user scopes by default**, verified: no `io.stat` here) [O/P] |
| Today / 7 d / 30 d | history `system_hours` sums + in-memory current bucket |
| Eject / unmount | UDisks2 `Filesystem.Unmount` / `Drive.Eject` (polkit) |

Visuals:
- read/write mirrored area chart
- per-device latency and util lines
- volume rows with capacity bars
- health card with status badge
- app table

### 2.6 Network

| Metric | Source |
|---|---|
| Per-interface counters | rtnetlink `RTM_GETLINK` dump with `IFLA_STATS64`: one syscall for all interfaces. Fallback: `/sys/class/net/*/statistics/*`. |
| Link change events | rtnetlink multicast groups `RTNLGRP_LINK`, `RTNLGRP_IPV4_IFADDR`, `RTNLGRP_IPV6_IFADDR` (event-driven, no polling) |
| Kind | `/sys/class/net/X/{type,uevent DEVTYPE}`, `wireless/` dir, `tun_flags`, `bridge/`, `lower_*`; `IFLA_LINKINFO` kind (wireguard, veth, bridge, tun, vlan) |
| Speed/duplex/MTU/MAC/state | `/sys/class/net/X/{speed,duplex,mtu,address,operstate,carrier}` |
| Addresses / routes | rtnetlink `RTM_GETADDR`, `RTM_GETROUTE` (default route) |
| DNS | `org.freedesktop.resolve1` (DNS property) if present → `/etc/resolv.conf` fallback |
| Wi-Fi | NetworkManager D-Bus (`AccessPoint.{Ssid,Strength,Frequency,MaxBitrate}`, `Device.Wireless.Bitrate`); fallback `/proc/net/wireless` + nl80211 (`NL80211_CMD_GET_STATION`) for signal dBm |
| Sockets / ports | `NETLINK_SOCK_DIAG` (`inet_diag_req_v2`) for TCP/UDP v4/v6: state, addresses, inode, uid, `tcp_info` (`tcpi_bytes_acked`, `tcpi_bytes_received`). All sockets are visible unprivileged; the inode → pid mapping needs `/proc/<pid>/fd` [O/P]. |
| Per-process traffic, unprivileged tier | Per TCP socket, Δ`tcpi_bytes_received` / Δ`tcpi_bytes_acked`, summed by owning pid → app. **TCP only.** Bytes from sockets that close between two ticks are lost (undercount). The table shows a "TCP only" badge. |
| Per-process traffic, helper tier | eBPF `cgroup_skb/ingress` + `egress` on the cgroup v2 root, counting bytes per cgroup id. This covers all protocols (incl. QUIC/UDP) and maps 1:1 onto app scopes. Optional fentry on `udp_sendmsg`/`udp_recvmsg`/`tcp_sendmsg`/`tcp_cleanup_rbuf` for per-pid detail. |
| Totals | history `system_hours`, plus `interface_hours` |

Visuals:
- download/upload mirrored area chart (download above the axis, upload below)
- per-interface sparkline rows
- Wi-Fi signal meter
- connections table
- listening-ports table with a loopback / LAN-exposed badge
- app table

---

## 3. Linux data-source reference

### 3.1 Access tiers

| Source | Unprivileged user | Needs |
|---|---|---|
| `/proc/stat`, `meminfo`, `vmstat`, `loadavg`, `pressure/*`, `diskstats`, `swaps`, `mountinfo` | ✅ | — |
| `/proc/<pid>/{stat,statm,status,cmdline,comm,cgroup}` | ✅ all processes (`cmdline` may be empty for kernel threads) | — |
| `/proc/<pid>/{io,environ,fd/,fdinfo/,smaps_rollup,exe,cwd}` | own UID only (ptrace access mode; Yama `ptrace_scope=1` here) | CAP_SYS_PTRACE (+DAC_READ_SEARCH) → helper |
| cgroup v2 `cpu.stat`, `memory.*`, `pids.*` | ✅ (world-readable) | — |
| cgroup `io.stat` | only where the io controller is enabled | — |
| `/sys/class/hwmon/*`, `/sys/class/drm/*`, `/sys/block/*`, `/sys/class/net/*` | ✅ mostly | — |
| `powercap` `energy_uj` | ❌ (0400) | root → helper |
| hwmon `pwm*` writes (fan control) | ❌ | root → helper |
| `perf_event_open` system-wide (i915 PMU, RAPL PMU) | ❌ (`perf_event_paranoid=1`) | CAP_PERFMON → helper |
| eBPF | ❌ (`unprivileged_bpf_disabled=2`) | CAP_BPF+CAP_PERFMON → helper |
| NVML | ✅ (device nodes are 0666) | — |
| NETLINK_SOCK_DIAG, rtnetlink dumps | ✅ | — |
| proc connector (process events) | ❌ | CAP_NET_ADMIN → not used; pid-set diff instead |
| UDisks2 SMART read | ✅ via polkit (usually allowed for active sessions) | — |

### 3.2 App identity sources

These sources drive the "apps, not processes" grouping:

1. **cgroup path** (`/proc/<pid>/cgroup`, one `0::` line on v2). Parse the systemd desktop-environment naming conventions (systemd.io/DESKTOP_ENVIRONMENTS):
   - `app-<launcher>-<appid>-<rand>.scope` and `app-<appid>@<rand>.service`. Verified here: `app-com.google.Chrome-77927.scope`, `app-orca-20154.scope`.
   - `app-flatpak-<appid>-<n>.scope`
   - `snap.<name>.<app>-<uuid>.scope`
   - terminal scopes (`kitty-<pid>-0.scope`)
   - `docker-<id>.scope` / `libpod-<id>.scope`: containers
   - `system.slice/<unit>.service`: System
   - Unescape `\x2d`.
2. **Desktop entry index**: `$XDG_DATA_DIRS/applications/*.desktop` + Flatpak exports. Maps app id, `StartupWMClass`, `Exec` basename and `TryExec` → name and icon. Built once and refreshed with inotify.
3. **Process ancestry**: walk ppid up to the nearest ancestor that matches an entry. Stop at `systemd --user`, the session leader or PID 1.
4. **Compositor**: `ext-foreign-toplevel-list-v1`, which crownpositor implements (`handlers/foreign_toplevel_list.rs`). It gives app ids of open windows (no pids), used to mark "has window" and to rank app candidates.
5. **Special groups**:
   - kernel threads (children of `kthreadd`, PID 2): **Kernel**
   - other UIDs, or system units: **System**
   - unmatched: the process name itself

**Browser and Electron split** (from `/proc/<pid>/cmdline`):
- Chromium/Electron: `--type=renderer|gpu-process|utility|zygote|crashpad-handler`, `--extension-process`, `--utility-sub-type=`
- Firefox: `-contentproc … tab|rdd|socket|gpu|utility|forkserver`
- Children are grouped as Tabs, Extensions, Graphics, Utilities and the browser itself.

---

## 4. Rust monitoring engine architecture

### 4.1 Workspace layout

The engine is split into one crate per domain. Crates can then be built, tested and developed in parallel, and a broken module in one domain never blocks another. Ownership and the shared `monitor-core` API are in `docs/conventions.md`.

```
crownos-process-monitor/
├── Cargo.toml                 # workspace + app package; workspace lints and dependencies
├── crates/
│   ├── monitor-core/          # ids, units, clocks, ring, collector trait, interest, fd index, util/{parse,sysfs,procfs}
│   ├── monitor-system/        # cpu, memory, processes, apps (grouping), inspect, actions
│   ├── monitor-io/            # disk (stats, volumes, SMART), net (links, sockets, Wi-Fi)
│   ├── monitor-hardware/      # gpu (amd, nvidia, intel, generic, accel), sensors, power
│   ├── monitor-history/       # rollups, SQLite store, sessions/gaps, queries, CSV export
│   ├── monitor-alerts/        # rules, detector, Worth a Look feed
│   ├── monitor-engine/        # sampler thread, interest scheduling, snapshot, per-app joins, live rings
│   └── monitor-helper/        # optional privileged D-Bus helper with modular Authorizer
└── src/                       # CrownUI app (anyhow): app/, pages/, components/, inspector/, settings/, util/
```

### 4.2 Threads

```
┌──────────── UI thread (CrownUI) ────────────┐     ┌──── render thread (CrownUI) ────┐
│ signals, layout, canvas painters            │ ──▶ │ Impeller / Vulkan, animations   │
└──────▲─────────────────────────┬────────────┘     └─────────────────────────────────┘
       │ Arc<Snapshot> (batched)   │ Interest / Command (channel)
┌──────┴─────────────────────────▼────────────┐
│ sampler thread (std::thread, blocking I/O)  │  owns all collectors and reusable buffers
└──────┬──────────────────────────────────────┘
       │ RollupBatch (once per minute)
┌──────▼──────────────┐   ┌───────────────────────────────────────────┐
│ history writer      │   │ D-Bus/tokio task (CrownUI executor):      │
│ thread (SQLite)     │   │ UDisks2, NM, logind, systemd, notifications│
└─────────────────────┘   └───────────────────────────────────────────┘
```

- **The sampler is a plain `std::thread`, not tokio.** `/proc` and sysfs reads are fast synchronous syscalls, and a dedicated thread keeps a stable cadence (`clock_nanosleep` to absolute deadlines) without blocking CrownUI's current-thread executor.
- **UI bridge.** It copies the `spawn_worker` pattern from `crownos-settings/src/util/worker.rs`: commands go down, events come up, and up to 64 events are batched per wake. Each tick produces one `Arc<Snapshot>`; the UI side diffs it into **fine-grained signals** (one per card or series), so only the nodes whose data changed re-record.
- **D-Bus slow paths** (SMART, Wi-Fi, units, inhibitors) are async tasks on CrownUI's tokio runtime via `zbus`. They post results into the sampler's command channel and never block a tick.

### 4.3 Collector model

```text
trait Collector {
    const CADENCE: Cadence;              // Fast(1s) | Medium(2s) | Slow(10s) | OnEvent | Once
    fn wanted(&self, interest: &Interest) -> bool;
    fn sample(&mut self, now: Instant, out: &mut SnapshotBuilder) -> Result<(), SourceError>;
}
```

- **Interest** comes from the UI: visible page, expanded app, inspected pid, selected GPU, window visible or minimized.
  - Expensive collectors run only when wanted: per-core frequency, fdinfo scans, PSS refresh, socket→pid mapping, PCIe throughput.
  - The **base set** always runs for history and alerts: CPU totals, memory, disk/net counters, process CPU/RSS, sensors.
- **Cadence:**
  - **1 s** while the window is visible (setting: 0.5–2 s)
  - **5 s** while the window is minimized or not visible. **Nothing samples once the app is closed** (D3): the engine lives inside the app process and stops with it.
  - static data **once**
  - mounts, links and hotplug **on event**: `POLLPRI` on mountinfo, rtnetlink multicast, udev netlink monitor for drm/block/net/hwmon
- **Errors:** `SourceError` (thiserror) is per collector and never fatal. A failing source is marked `Unavailable(reason)` in the snapshot, and the UI shows the hardware-dependent empty state.

### 4.4 Process table (hot path)

- Enumerate with `getdents64` on an open `/proc` dirfd, via rustix, without allocating names.
- For each pid, `openat(proc_dirfd, "<pid>/stat")`. **Keep the per-process `stat` fd open** and re-read it with `pread(fd, buf, 0)`; seq_file regenerates the content. This saves an open/close pair per process per tick.
  - Raise `RLIMIT_NOFILE` soft to hard.
  - Close the fd when the process ends (`ESRCH`/`ENOENT` on read).
- **PID-reuse safety:** the key is `(pid, starttime)`, where starttime is stat field 22.
- One reusable 4 KiB buffer per read. Parsers work on `&[u8]` with `memchr`. `comm` is parsed from between the *last* `)` and the first `(`, because process names may contain spaces and parentheses.
- Read once per process lifetime (cached): `cmdline`, `cgroup`, `exe`, `uid`. Re-read on `execve`, detected when the `comm` field changes.
- Per tick: `stat` (CPU, threads, state, faults) and `statm` (RSS). `status` is only for inspected or visible rows.
- Process records live in a slab `Vec<ProcessEntry>` with a `HashMap<Pid, SlotIndex>`. Slots are reused, so there is no steady-state allocation.
- Ended processes move to an `EndedRecently` ring (10 min, capped).

### 4.5 App aggregation

On each tick, after the process sweep:

1. Resolve each process's `AppId`. This happens once per process (cached), using the ordering in §3.2.
2. Aggregate into an `AppEntry` slab: CPU, memory (the best method in §2.4), I/O, network, GPU, process count, and the split into kinds.
3. Sort per page metric. Pages receive **sorted index vectors**, so the UI never sorts.

### 4.6 GPU subsystem

- `GpuBackend` is an enum, not a trait object, for zero-cost dispatch: `Amd`, `Nvidia(NvmlHandle)`, `IntelI915`, `IntelXe`, `Generic`, `Nouveau`.
- `RuntimePmGuard` is checked before every device sample (§2.3).
- **NVML** is loaded with `nvml-wrapper`, which `dlopen`s `libnvidia-ml.so.1` at runtime, so there is no link-time dependency.
  - Initialised lazily on the first active NVIDIA device and shut down when every NVIDIA device is asleep. Verify on hardware that an open NVML context doesn't block D3cold.
  - `nvmlDeviceGetProcessUtilization` takes the last-seen timestamp to get deltas.
- **fdinfo scanner:**
  - For own processes, scan only pids whose fd table changed. Track a per-pid DRM fd set, rebuilt when `/proc/<pid>/fd` is newer than the cached version, or every 10 s.
  - Runs only when the GPU page or per-app GPU column is visible, or for the history base set every 5 s.
- **AMD `gpu_metrics`:** a binary blob with a header `(structure_size, format_revision, content_revision)`. Implement a hand-written parser for the v1.x/v2.x/v3.x layouts we need (temperatures, activity, power, clocks). The fallback is the plain sysfs files.

### 4.7 Network sockets

- One sock_diag dump per tick per family/protocol (TCP v4/v6, UDP v4/v6) using a reusable receive buffer.
- `inode → (pid, fd)` map:
  - rebuilt incrementally: only new inodes trigger a `/proc/*/fd` walk, limited to own processes
  - full rebuild every 10 s, and only while the Network page or the per-app network column is visible
- Per-socket byte counters are kept between ticks to compute deltas. Socket entries disappear when the socket closes.

### 4.8 Actions

| Action | Mechanism | Safety |
|---|---|---|
| Quit process | `pidfd_open` + `pidfd_send_signal(SIGTERM)` (no PID-reuse race) | confirmation dialog |
| Force quit | `SIGKILL` via pidfd | confirmation dialog |
| Quit app | signal every member (pidfd each), or `StopUnit` on the app scope via systemd user D-Bus | dialog shows "N processes will close" |
| Force quit app | write `1` to the scope's `cgroup.kill` (owner-writable on delegated cgroups) | dialog |
| Pause/resume app | `cgroup.freeze` 1/0 on the scope (atomic for the whole app); per process: SIGSTOP/SIGCONT | dialog for pause |
| Renice | `setpriority` (raising priority needs CAP_SYS_NICE → helper, `SignalOtherUsers`-class capability) | — |
| Other users' processes | through the helper; authorized by the active `Authorizer` (polkit by default, prompt via `crownos-polkit-agent`) | auth dialog |

**Refuse list ("leave it alone"):** PID 1, kernel threads, the monitor itself, the compositor (`crownpositor`), `systemd --user`, the session D-Bus and the login session leader. Refusals are explained in the dialog.

### 4.9 Optional privileged helper (`monitor-helper`)

This mirrors Vitals' "optional helper": everything works without it, and it only fills in the [P] cells. Shipping it was approved (D2), with authorization kept **modular** so polkit can be swapped for a CrownOS-specific mechanism.

**Layout:**

```
crates/monitor-helper/
└── src/
    ├── main.rs            # D-Bus service, idle exit
    ├── service.rs         # method handlers; calls authorize() before every privileged method
    ├── capability.rs      # enum Capability { ReadAllProcesses, ReadEnergy, ReadNetworkAccounting,
    │                      #                   ReadGpuCounters, ControlFans, SignalOtherUsers }
    ├── auth/
    │   ├── mod.rs         # trait Authorizer + Decision + selection
    │   ├── polkit.rs      # PolkitAuthorizer (feature "polkit", default)
    │   └── allow_list.rs  # StaticAuthorizer: uid/group policy file, for development and tests
    ├── sources/           # all-user /proc reads, RAPL, PMU (reuse monitor-engine parsers)
    ├── bpf/               # aya loader + cgroup_skb programs (feature "ebpf")
    └── fans.rs            # pwm set/restore, restore on client disconnect
```

**The authorization boundary** is one trait that the service depends on and never sees past:

```text
trait Authorizer: Send + Sync {
    async fn authorize(&self, caller: &Caller, capability: Capability) -> Result<Decision, AuthError>;
}
struct Caller { uid: u32, pid: u32, pidfd: OwnedFd, bus_name: String }
enum Decision { Allowed, Denied, Challenge }
```

- `Caller` is built by the service from D-Bus credentials (`GetConnectionCredentials`: UID, PID and a pidfd, so a recycled PID can't impersonate a caller). Backends never read the D-Bus message themselves.
- `Capability` is a closed enum. Each D-Bus method maps to exactly one capability in a single table in `service.rs`, so a new backend can't miss a method.
- **Selection** is a cargo feature plus one constructor in `auth/mod.rs`: `polkit` (default) or a future `crownos` backend. Replacing polkit means adding `auth/crownos.rs`, implementing the trait and switching the feature. Nothing else changes.
- **Decisions are cached** per `(caller pidfd, capability)` for the lifetime of the caller's connection, so read methods called every tick don't re-authorize.
- `PolkitAuthorizer` calls `org.freedesktop.PolicyKit1.Authority.CheckAuthorization` with subject `unix-process` (pidfd-based where polkit supports it). Actions are declared in `org.crownos.process-monitor.policy`:
  - `read-metrics` (allowed for active local sessions, no prompt)
  - `control-fans` (admin, prompt)
  - `signal-other-user` (admin, prompt)

**Client side.** The app talks to the helper through `monitor-engine/src/source/privileged.rs`, a typed proxy. It reports helper availability (absent, denied, available) so pages can show [O] badges, and it doesn't know which authorization backend is in use.

**Activation and transport:**
- A system D-Bus service (`org.crownos.ProcessMonitor1`), D-Bus/systemd activated.
- It exits when its last client disconnects (plus 30 s idle). With D3 there is no background client, so the helper only runs while the app is open.

**Read-only methods:**
- per-pid io / PSS / fd / fdinfo for all UIDs, batched: one call per tick returns a compact binary table
- RAPL energy counters
- eBPF per-cgroup network byte counters
- i915/RAPL PMU readings

**Write methods:**
- Fan `pwm` set/restore. The helper itself restores automatic mode when the client disconnects (it watches the D-Bus name owner), so fans never stay fixed after a crash.
- Signals to other users' processes.

It reuses `monitor-engine` parsers. The eBPF path uses `aya`.

---

## 5. CrownUI components

### 5.1 Reused as-is (verified in `crownui/widgets/src/kit`)

| Need | CrownUI |
|---|---|
| Sidebar with 6 pages + live trailing values | `sidebar_with_header`, `sidebar_item(..).trailing_text(..)`, `sidebar_section` |
| Page switching | `slide_transition(index_signal, ..)`, the same as `crownos-settings/src/app/root.rs` |
| Cards / panels | `card`, `pane`, `section`, `divider`, `soft_shadow` |
| Segmented controls (5/15/30 min, GPU selector, Live/History) | `toggle_group` |
| Range dropdown, unit selects | `select` |
| Find field | `search` / `text_input` |
| Context menu | `context_menu_at` + `menu_item`/`menu_separator`/`menu_submenu` |
| Confirm dialogs | `dialog` |
| Tooltips | `tooltip` |
| Capacity bars, stacked memory/VRAM bars | `progress`, `segmented_bar` |
| Status badges (pressure, health, "TCP only", "Asleep") | `badge`, `status`, `tag_chip`, `callout` |
| Toasts (action results) | `toast_host` / `use_toasts` |
| Virtualized tables | `lazy_list(count, revision, row_height, row)` + `ScrollHandle` (built for 100k rows) |
| Icons | Lucide via `kit::icon(icons::…)`; app icons via `svg` / image |
| Theme | `provide_configured_theme(cx)`: follows CrownOS appearance, light/dark, accent tokens |
| Motion | `.transition(Spring::snappy())`, `.enter()/.exit()` |

### 5.2 New components to build in `src/components/`

They are built on `canvas` / `cx.set_canvas` and the `DrawList` API: `FillPath`/`StrokePath`, `Path::{move_to,line_to,cubic_to,close}`, `Brush::LinearGradient`, `Stroke::rounded`, `clip`, `save`/`restore`.

| Component | Design |
|---|---|
| `area_chart` | Gradient-filled area + stroke line. Data comes from a ring buffer slice; **M4 decimation** to pixel columns keeps the path at ≤ 2 × width vertices. Optional reference line (Tjmax, 90 %). Null samples (suspend gaps) break the path. |
| `line_chart` | Multi-series lines (per engine, per interface) with legend toggles |
| `stacked_area_chart` | CPU states, memory categories |
| `mirrored_area_chart` | Disk read/write and network down/up |
| `bar_chart` | Weekly bars beyond 90 d, hour strip, fan bars |
| `ring` | Donut with N segments. Arcs from a local copy of `arc_path` (it is `pub(crate)` in `widgets/src/util/arc.rs`; either copy it into `src/util/arc.rs` or ask to make it `pub` in CrownUI). Gaps between segments, hover highlight. |
| `chart_overlay` | Axis labels, crosshair and value tooltip as `text` nodes in a `zstack` over the canvas. The canvas has no text system. |
| `core_grid` | `grid` of mini canvases, one per CPU, grouped by core type |
| `app_table` | Sticky header row (sortable columns, sort indicator, Columns menu) + `lazy_list` body; expandable app rows into process rows (flattened index model); keyboard navigation; selection model; right-click. Row height fixed (`lazy_list` requirement). |
| `drawer` | Right-side sheet: `zstack` + `absolute` + `.enter`/`.exit`. Charts move aside instead of being covered. |
| `stat_strip` | Headline value + secondary figures with units |
| `range_picker` | `toggle_group` (live) / `select` (history) + Export button |
| `capacity_bar` | Volume and VRAM usage with threshold colouring |

Rendering rules:
- **One canvas per chart.** CrownUI caches draw lists per node boundary and re-records only nodes whose signals changed (`runtime/src/render/tree.rs`).
- Charts update **once per sample**, not every frame. Smooth scrolling (`request_animation_frame`) is optional and off on battery.
- Pages and sections that aren't visible don't subscribe to signals. Check whether `slide_transition` keeps off-screen pages mounted; if it does, gate painters on a `page_visible` memo.
- CrownUI has no partial-surface damage (`SurfaceState.damage` is unused), so keep the number of changing nodes per tick small.

### 5.3 Gaps to raise upstream in CrownUI (optional)

- a public `arc_path` helper
- a table/data-grid primitive
- a drawer/side sheet
- a resizable splitter
- dash patterns for reference lines (workaround: short segments)

---

## 6. Data collection and history storage

### 6.1 Live tier (in memory)

- `RingSeries<T>`: a fixed-capacity, struct-of-arrays ring with a timestamp ring plus value rings. It is allocated once and never grows.
- System series: 1 s × 30 min = 1800 samples. About 40 series × 1800 × 4 B (f32) ≈ **290 KB**.
- Per-app series only for the top 32 apps + pinned apps + the inspected process: 4 metrics × 1800 × 4 B × ~34 ≈ **1 MB**. They are evicted when an app leaves the set.
- The UI reads ring slices by `Arc`-shared snapshot handles (copy-on-publish of the tail only), so it never locks the sampler.

### 6.2 Rollup tier

Counters are kept as **mergeable sums**, not averages, so every rollup level is exact. This is the same design as Vitals' `cpu_load_seconds` and `memory_byte_seconds`:

- `cpu_seconds`
- `memory_byte_seconds` (average = sum ÷ seconds)
- `bytes_read` / `bytes_written`, `bytes_in` / `bytes_out`
- `energy_joules`
- peaks: max per bucket

An in-memory accumulator builds minute buckets. Each minute it hands a `RollupBatch` to the history writer thread.

### 6.3 Persistent tier (SQLite)

- **File:** `$XDG_STATE_HOME/crownos-process-monitor/history.sqlite` (default `~/.local/state/...`).
- **Pragmas:** WAL, `synchronous=NORMAL`, `journal_mode=WAL`, `temp_store=MEMORY`, `cache_size` small (−2000 = 2 MB).
- **Writes:** one transaction per minute with prepared statements cached via `prepare_cached`.
- **Tables:** `STRICT`, `WITHOUT ROWID`, primary key `(start[, id])`, Unix-epoch UTC seconds, unit in each column name.

| Table | Row every | Kept | Content |
|---|---|---|---|
| `readings` | 5 min | retention | avg/peak CPU, memory, pressure, swap, GPU (max over GPUs), CPU/hottest °C, fan rpm, power W, throttled seconds, battery %, on-AC seconds |
| `system_minutes` | 1 min | 3 h | cpu_seconds, memory_byte_seconds, net in/out, disk r/w, energy_joules |
| `system_five_minutes` | 5 min | 48 h | same |
| `system_hours` | 1 h | retention | same |
| `gpu_hours` | 1 h per GPU | retention | busy_seconds, vram_byte_seconds, energy_joules, peak °C |
| `device_hours` / `interface_hours` | 1 h per disk/iface | retention | bytes r/w, in/out |
| `app_hours` | 1 h per app | 48 h | cpu_seconds, memory_byte_seconds, peak_memory, io r/w, net in/out, gpu_seconds, energy_joules (est.) |
| `app_days` | 1 d per app | retention | same |
| `apps` | — | — | app_id ↔ name, kind, desktop id, icon (dimension table, so app rows store an integer) |
| `alerts` | event | retention | type, app, started, ended, peak, count, quieted |
| `sessions` | app run | retention | started, ended, boot_id |
| `gap_buckets` | app start after a gap | retention | from, to, cpu_seconds, disk r/w, net in/out, estimated flag |

- **Retention:** 30 d (default), 90 d, 6 mo or 1 y. Pruned once a day (`DELETE … WHERE start < ?` per table, then `PRAGMA incremental_vacuum`). Estimated size for 1 year at about 100 apps is **~25–40 MB**.
- **Queries:**
  - Today / 7 d / 30 d totals = Σ `system_hours` + the live current-hour accumulator.
  - Ranges ≤ 3 h use `system_minutes`; ≤ 48 h use `system_five_minutes`; above that, `system_hours`; beyond 90 d, weekly aggregation in SQL.
- **CSV export:** three files, the same as Vitals (apps by day, system by hour, readings every 5 min), written by a hand-rolled writer on the writer thread into `$XDG_DOWNLOAD_DIR`.
- **Delete history:** truncate the tables and record `recording_since`.

### 6.4 Coverage while the app is closed (D3: no background sampling)

History is recorded only while the app is running. To keep totals honest:

- **`sessions` table**: `(started, ended, boot_id)`, one row per app run. `boot_id` comes from `/proc/sys/kernel/random/boot_id`. The ended time is updated with each minute flush, so a crash loses at most one minute.
- **Charts** draw gaps for time that wasn't recorded. They never interpolate across them.
- **Counters since boot carry the gap.** The kernel's CPU, disk and network counters run from boot whether or not the app is open. At startup, if `boot_id` matches the last session, the engine computes `current − last_persisted` per counter. That delta is stored as one **gap bucket** covering the closed interval, marked `estimated`, so "today", "7 days" and "30 days" totals for CPU time, disk and network stay correct. Its distribution over time is unknown, so charts show it as a hatched gap, not a curve. On a different `boot_id`, the counters before the reboot are lost and the gap is labelled "not recorded".
- **Per-app figures, Peaks, At a Limit and the busiest-by-hour ranking cover recorded time only.** The UI states the recorded share of the range (e.g. "recorded 9 h 40 m of 24 h").
- **Alerts run only while the app is open.** Worth a Look keeps past alerts across restarts.
- **Average today** is the mean over recorded time today, and is labelled that way.

### 6.5 Time correctness

- Rates use `CLOCK_MONOTONIC`. Gaps are detected by comparing `CLOCK_BOOTTIME` with `MONOTONIC` deltas (system suspend): a gap is stored as no data, never as a spike.
- Buckets are aligned to UTC wall time. A wall-clock jump starts a new bucket and is never merged backwards.
- **Counter resets** (interface re-created, disk hot-plugged, nvidia driver reload, process exec): any negative delta is treated as a reset, and the sample is skipped.

---

## 7. Performance strategy

**Budgets** (to be measured in CI with a benchmark harness on fixtures plus a soak test on hardware):

| State | Target |
|---|---|
| Window minimized, 5 s cadence, ~700 processes | ≤ 0.1 % of one core (Vitals' figure), 0 wakeups between ticks |
| Window visible, 1 s cadence, CPU page | ≤ 1 % of one core for the engine; UI frame work only on tick |
| Engine heap, steady state | ≤ 8 MB; zero allocations per tick after warm-up (asserted with a counting allocator in tests) |
| Process RSS, total | ≤ 60 MB (Impeller/Vulkan driver memory dominates; measured, not assumed) |
| History DB writes | 1 transaction/min, ≤ 1 fsync/min |

**Techniques:**

1. **Persistent fds and `pread`** for every hot file: `/proc/stat`, `meminfo`, `diskstats`, `vmstat`, `pressure/*`, per-process `stat`, sysfs counters, hwmon inputs. Hot ticks then do no `open` syscalls.
2. **Zero-copy byte parsers** on reusable buffers. No `String`s and no `format!` in the sampler. Names are interned once per process and app (`Arc<str>`). Cow-style borrowing from buffers for transient parsing.
3. **Interest-driven collection.** Expensive sources run only while visible (§4.3). PSS is refreshed round-robin; socket→pid and DRM fd maps are refreshed incrementally.
4. **Event-driven instead of polling** for mounts, links/addresses, hotplug (udev netlink) and desktop files (inotify).
5. **No subprocesses, ever.** No `nvidia-smi`, `smartctl`, `lsblk`, `ip`, `ss` or `systemctl`: NVML, UDisks2/D-Bus and netlink replace them all.
6. **Non-blocking UI.**
   - The sampler and history writer run on their own threads; D-Bus work runs as async tasks.
   - The UI thread only diffs a snapshot into signals.
   - Sorting and filtering happen on the sampler side. Regex compilation happens once per filter change.
7. **Charts:**
   - M4 decimation bounds the vertex count by pixel width.
   - One canvas per chart; repaint once per sample.
   - No per-frame animation by default.
   - Gradient fills use the native `LinearGradient` brush, so they're GPU-rasterised by Impeller.
8. **GPU-friendly sampling.**
   - Never wake a suspended dGPU.
   - Slow NVML calls (PCIe throughput) only every 5 s.
   - `gpu_metrics` as one read instead of ~10 sysfs files.
9. **Allocation discipline.** Slabs with reusable slots, `Vec::with_capacity` sized from the previous tick, `clear()` instead of re-allocating, `SmallVec`-like fixed arrays for per-core data sized once at startup (CPU hotplug triggers a resize).
10. **Release profile:** `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip`. These match the CrownUI workspace profile.
11. **Benchmarks:** `criterion`-free micro-benchmarks via `cargo bench` with std `Instant` on recorded fixtures (keeps dependencies minimal), plus a `perf stat` soak script.

---

## 8. Dependencies and trade-offs

| Crate | Version (crates.io, verified) | Use | Trade-off / alternative |
|---|---|---|---|
| `crownui` | path `../crownui/crownui`, features `wayland, impeller, static, tokio` | UI (mandatory) | — |
| `crownos-config`, `crownconfig` | path | settings persistence, live watch (RON under `~/.config/crownos/`) | consistent with `crownos-settings` |
| `tokio` | 1.54 (`rt, sync, time, macros`) | already in CrownUI's executor; D-Bus tasks | the sampler deliberately does **not** use it |
| `rustix` | 1.1 | `getdents64`, `openat`, `pread`, `statvfs`, `readlinkat`, `pidfd_open`/`pidfd_send_signal`, `setpriority`, `prlimit`, netlink sockets | **chosen** over `nix` 0.31 (heavier, allocating APIs) |
| — (own parsers) | — | `/proc` and sysfs parsing | **rejected `procfs` 0.18** (allocates a `String` per read; fine for one-offs but not the hot path) and **`sysinfo` 0.40** (cross-platform lowest common denominator: no fdinfo, cgroup, PSI or zram; refreshes more than needed; allocation-heavy) |
| `memchr` | 2.x | fast field scanning | tiny, already transitive in most trees |
| `nvml-wrapper` | 0.13 | NVIDIA metrics, per-process | dlopens `libnvidia-ml.so.1` at runtime, so the app runs without NVIDIA; alternative raw FFI is not worth it |
| — (own `gpu_metrics` parser) | — | AMD | **optional `libdrm_amdgpu_sys` 0.9** for ioctl sensors/marketing name; it adds a libdrm dependency. Marketing names come instead from parsing `/usr/share/libdrm/amdgpu.ids` |
| `zbus` | 6.2 | UDisks2, NetworkManager, resolved, logind inhibitors, systemd units, UPower, notifications | pure Rust, async on tokio; longer compile time vs C `dbus` (rejected: C dependency, blocking) |
| `netlink-packet-route` + `netlink-packet-sock-diag` 0.5 + `netlink-sys` 0.9 | current | rtnetlink link/addr/route dumps and events, sock_diag | ready-made message types; alternative: hand-rolled structs over rustix sockets (fewer deps, more code). Start with the crates and revisit if binary size matters. |
| `rusqlite` | 0.41 (system `libsqlite3`; `bundled` optional) | history store | **chosen**: users can open the file (Vitals parity), SQL rollups, mature. Rejected `redb` 4.4 (pure Rust but not user-queryable, custom rollups) and custom mmap'd binary rings (fastest, but a schema and tool burden) |
| `raw-cpuid` | 11.6 | x86 model, caches, core types, flags | x86 only; Arm path uses `/proc/cpuinfo` |
| `regex` (or `regex-lite`) | 1.13 | `/pattern/` filter | `regex-lite` is smaller and slower: fine for ~1000 rows. Prefer `regex-lite`. |
| `thiserror` | 2.0 | engine errors | per user rules |
| `anyhow` | 1.0 | app errors | per user rules |
| `serde` | 1.0 | settings sections (via crownos-config) | already required |
| `aya` | 0.16 | helper-only eBPF network accounting | pure Rust, CO-RE, no clang at build time for the loader (programs still need a BPF target build); alternative `libbpf-rs` 0.27 needs clang + libbpf |
| — | — | pci.ids, desktop entries, icon theme lookup, CSV | hand-written in `util/`; each is < 150 lines and avoids crates (`pci-ids`, `freedesktop-desktop-entry`, `csv`). Icon lookup: check for a CrownOS helper first (`crownos-icon-theme`). |

---

## 9. Hardware compatibility and edge cases

### 9.1 Compatibility matrix

| Area | Full | Partial | Fallback |
|---|---|---|---|
| CPU util / per core / states | all | — | — |
| CPU frequency | `amd-pstate`, `intel_pstate`, `acpi-cpufreq`, `cppc` | VMs (often none) | hide |
| CPU temperature | k10temp, zenpower, coretemp, Arm `cpu_thermal` | laptop EC drivers vary | `acpitz` thermal zone, labelled "ACPI" |
| Fans | `legion_hwmon`, `thinkpad`, `dell_smm`, `asus`, `nct67xx`, `it87` | read-only on many laptops | hidden if no `fanN_input` |
| Fan control | hwmon `pwmN_enable` writable | vendor ECs with profiles only | show platform profile instead |
| Power | battery `power_now`; RAPL (Intel/AMD Zen) via helper | ARM SoCs | "unknown" |
| AMD GPU | amdgpu GCN 1.2+ (sysfs + fdinfo + `gpu_metrics`) | radeon (legacy): hwmon only | — |
| NVIDIA | proprietary and open modules with NVML | nouveau: temperature/memory only | — |
| Intel GPU | i915 (Gen8+), xe (Lunar Lake/Battlemage+) | Arc on i915: VRAM via fdinfo only | — |
| ARM GPUs | panfrost/panthor/msm/v3d via fdinfo | asahi: varies | devfreq load |
| NPU | `intel_vpu`, `amdxdna` | — | section hidden |
| SMART | UDisks2 ≥ 2.10 (NVMe) | USB bridges hide SMART | "health unavailable" |
| Per-app net | helper eBPF (all protocols) | unprivileged: TCP only, own processes | badge |
| Per-app disk | own processes, or helper | — | badge |

### 9.2 Edge cases to handle explicitly

- **Hybrid graphics:**
  - dGPU in D3cold is never woken (§2.3).
  - PRIME offload apps are mapped by `drm-pdev` / NVML pid.
  - eGPU hot-plug and driver unbind/rebind arrive via udev events, and the backend is rebuilt.
  - Several GPUs from the same vendor are told apart by PCI BDF, never by card index (card numbers can change across boots).
- **NVML:**
  - `NVML_ERROR_GPU_IS_LOST`, `NOT_SUPPORTED` per field
  - per-process memory `NOT_AVAILABLE` in containers or pid namespaces
  - pid namespace mismatch for Flatpak apps: map through `NSpid` in `/proc/<pid>/status`
- **Processes:**
  - zombie (`Z`) and kernel threads (empty `cmdline`)
  - names with spaces or `)`
  - pid reuse (key includes starttime)
  - extremely short-lived processes (missed between ticks, acceptable; Ended Recently only covers seen ones)
  - exe `(deleted)` after upgrades
  - Flatpak/bwrap pid namespaces (`NSpid`)
  - processes of other users (partial data shown, flagged)
  - `hidepid=2` mounts of `/proc` (only own processes visible: show a notice)
- **Containers:** docker/podman scopes as apps; rootless podman under the user slice; LXC/systemd-nspawn via `machine.slice`.
- **Memory:**
  - no swap
  - zram + disk swap together
  - zswap + zram double compression (show both)
  - PSI disabled (`psi=0` / `CONFIG_PSI=n`): fall back to the available ratio
  - cgroup v1 hybrid systems: no per-app cgroup memory → PSS/RSS methods
  - huge pages reserved but unused
- **Disk:**
  - btrfs (shared space between subvolumes; `statvfs` approximations)
  - bind mounts (dedupe by `st_dev` + root)
  - LUKS/LVM/MD/bcache stacking
  - loop devices for snaps (hidden)
  - network filesystems: never `statvfs` on stale NFS (do it on a background thread with timeout, or skip remote mounts by default)
  - removable media appearing and disappearing
  - `diskstats` sector unit is always 512 B
- **Network:**
  - interface renamed or re-created (counter reset)
  - VPN/tunnel double counting (physical NIC + tun carry the same traffic): totals count **physical + non-tunnel** interfaces only, with tunnels shown separately
  - veth/bridge traffic of containers excluded from totals
  - 32-bit counters on some drivers (use `IFLA_STATS64`)
  - Wi-Fi without NetworkManager (iwd/wpa_supplicant only): nl80211 fallback
  - IPv6-only hosts
- **Sensors:**
  - hwmon numbering changes across boots (key by `name` + device path)
  - duplicate driver instances (two `nvme`)
  - `-273 °C` / `0` / `127 °C` sentinel values filtered
  - laptop ECs that are slow to read (cap read time; move to the slow cadence)
- **Time:** suspend/resume gaps, DST and timezone change (UTC storage, local display), clock jumps.
- **Virtual machines:** no sensors, no cpufreq, steal time significant (shown), virtio disks.
- **CPU hotplug and SMT toggle at runtime:** resize per-core arrays on the uevent.
- **Large systems:** 256+ CPUs (core grid switches to a heatmap), 10k+ processes (lazy list + sampler-side sort keep it O(n)).

---

## 10. Implementation roadmap

Each phase ends with `cargo fmt --check && cargo clippy -- -D warnings && cargo test`. If you ask for commits, there is one per milestone.

| # | Phase | Delivers | Depends on |
|---|---|---|---|
| **P0** | Foundations | Workspace (`crates/monitor-engine`, app crate), lints (`unwrap_used`/`expect_used` warn), `util/parse.rs` byte parsers, `util/sysfs.rs` persistent-fd reader, fixture capture script and recorded `/proc` + `/sys` fixtures from this machine, counting-allocator test harness | — |
| **P1** | Engine core | Sampler thread with cadence and `Interest`, `Snapshot`, CPU (totals, per core, states, load, PSI, counters), memory (meminfo, vmstat, swaps, zram, PSI), process table hot path (§4.4), `EndedRecently`, monotonic/boottime gap handling, benchmarks on fixtures | P0 |
| **P2** | App shell | CrownUI window, `provide_configured_theme`, sidebar with the 6 pages + trailing values, `slide_transition`, engine↔UI bridge (worker pattern), `util/format.rs`, placeholder pages, settings dialog skeleton (units, interval) | P0 (in parallel with P1) |
| **P3** | Components | `area_chart` (+ M4 decimation), `line_chart`, `stacked_area_chart`, `mirrored_area_chart`, `bar_chart`, `ring`, `chart_overlay`, `stat_strip`, `range_picker`, `app_table` (sort, expand, columns, find/regex, context menu), `drawer`, `confirm`. Headless tests with `crowntest`. | P2 |
| **P4** | App grouping | cgroup name parser, desktop index + inotify, ancestry resolution, kernel/system/container groups, browser/Electron split, app icons, per-app aggregation with the best memory method | P1 |
| **P5** | CPU + Memory pages | Full pages per §1.3 / §1.5 (without sensors, fans, power, DIMM history); app tables live | P1, P3, P4 |
| **P6** | Sensors and power | hwmon discovery/classification (adapted from `crownbar/.../stats/hwmon.rs`), temperatures, fans, battery, power profile, logind inhibitors; CPU page sensors section; DIMM panel | P1, P5 |
| **P7** | Disk | diskstats, mountinfo watcher (adapted from `crownos-files` mounts), statvfs, device stack, UDisks2 SMART and NVMe health, drive temperatures, per-process I/O [O], Disk page | P1, P3, P4 |
| **P8** | Network | rtnetlink links, addrs, routes and events, interface kinds, NM Wi-Fi + nl80211 fallback, DNS, sock_diag connections and listening ports, per-app TCP traffic [O], Network page | P1, P3, P4 |
| **P9** | GPU | Discovery + PCI names + runtime-PM guard, AMD (sysfs + `gpu_metrics` + fdinfo), NVIDIA (NVML incl. per-process), Intel i915 and xe, generic fdinfo, NPU section, GPU page with selector; hybrid testing on this laptop | P1, P3, P4, P6 |
| **P10** | History | Rollup accumulators, SQLite store and schema, writer thread, retention, sessions + gap buckets (§6.4), Live/History switch on every page, range queries, today/7 d/30 d totals, CSV export, delete history | P1 (engine side); P5–P9 (page integration) |
| **P11** | Overview | Subsystem cards, rings (by type, by app, CPU by app, power by app), Over Time, busiest-by-hour, Peaks, At a Limit, awake inhibitors | P5–P10 |
| **P12** | Inspector and actions | Inspector drawer (all sections in §1.8: security, systemd unit, ELF, children, sockets, GPU, live charts), export text/CSV, quit/force/pause/renice/app-scope actions with refuse list and confirmations | P3, P4 (P7–P9 for the disk/net/GPU sections) |
| **P13** | Alerts | Rule engine (sustained CPU, memory growth, disk/net burst vs EWMA baseline, heat, GPU ≥ 90 %, per-app custom limits, cooldown), Worth a Look feed, notifications via `org.freedesktop.Notifications` (crownotify), red-under-strain states, settings for thresholds and quieted apps | P10, P11 |
| **P14** | Privileged helper | `monitor-helper` D-Bus service, `Authorizer` trait with the polkit backend + static test backend and policy file, all-user per-process io/PSS/fdinfo, RAPL energy, eBPF per-cgroup network (aya), i915 PMU, fan control with crash-safe restore, other-user signals; UI badges switch from [O] to full | P7, P8, P9, P6 |
| **P15** | Hardening and polish | Performance budget verification (§7) and soak tests; hardware matrix runs (Intel iGPU, Intel Arc, NVIDIA-only desktop, VM, ARM if available); accessibility labels; keyboard shortcuts (Ctrl+F, Ctrl+1…6, Delete = quit with confirm); palettes (monochrome, colour-blind safe), compact mode; image export of Overview | all |

**Critical path:** P0 → P1 → P4 → P5. P2 → P3 runs in parallel with P1. Once P5 is done, P7, P8 and P9 can proceed in parallel. P10 → P11 → P13. P14 can start any time after P9.

**Definition of done for each page:**
- every checklist item in §1 for that page is either implemented, or shows its hardware-dependent empty state
- it is covered by fixture tests
- it stays within the §7 budget with the page open

---

## 11. Decisions

| # | Question | Decision | Effect on the plan |
|---|---|---|---|
| D1 | Settings surface | Header gear → settings **dialog** | §0.3; settings dialog in P2 (skeleton) and P15 (complete) |
| D2 | Privileged helper | **Yes**, with modular authorization | §4.9: `Authorizer` trait, polkit as the default backend behind a feature, replaceable by a CrownOS backend without touching the service |
| D3 | Background sampling when closed | **No** | §4.3 cadence, §6.4 sessions and gap buckets, §7 budgets; alerts and history only while the app runs; helper lifetime tied to the app |
| D4 | Package provenance in the Inspector | **Removed** | No Provenance section, no package database readers, no Version column. Sandbox identity (Flatpak id / Snap name) stays in Security. |
| D5 | i18n | **English only** for v1 | No i18n layer; strings stay in the UI crate |
