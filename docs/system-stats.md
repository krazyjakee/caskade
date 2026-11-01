# System statistics

Settings → Statistics (#156) shows how the machine Caskade runs on is doing:
CPU, GPU and memory use, network rates and storage capacity. The core samples
(`caskade_core::system_stats`), the adapter forwards samples
(`apps/desktop/src-tauri/src/commands/system_stats.rs`) and the dashboard
draws them (`apps/desktop/src/views/settings/statistics/`).

## Sampling and overhead

- One sample a second (`INTERVAL`); the history keeps the last 60 (`HISTORY`),
  so graphs cover a minute. The history is only in memory.
- Sampling runs only while someone watches. The dashboard takes a watch with
  `system_stats_watch` when it opens and the window is visible, renews it every
  10 s, and gives it back with `system_stats_unwatch` when it closes or the
  window is hidden. A watch that isn't renewed lapses after 30 s (`LEASE`), so a
  reloaded or crashed webview doesn't keep the sampler running.
- Each sample reads on a blocking thread, never on Tokio's workers or the UI
  thread, and outside the shared job queue, like process monitoring: the
  dashboard must not wait behind a library scan. Volumes are re-listed every
  5 samples. Each sample reaches the webview as a `SystemStatsSampled` event.
- Opening the dashboard again within a minute shows the history so far, with a
  gap for the time it was closed; an older history is dropped.

## Never a made-up zero

A reading that couldn't be taken is missing, not 0:

- CPU usage is measured between two readings, so a watch's first sample has
  none; the dashboard says "Measuring…".
- A sample whose read failed (the reader panicked) has `error` set and every
  metric empty. Graphs show it as a gap, and storage keeps the last volumes.
- Network rates need two readings of an interface's counters. A new interface,
  or one whose counters reset, has no rate until its next sample.
- A GPU whose driver doesn't report usage, or nvidia-smi that is missing or
  failing, gives the GPU a reason instead of a value.

The dashboard's states: loading (no sample yet), live, stale (no sample for
three intervals, or the last renewal failed) and error (watching failed and
there is nothing to show). It keeps trying in every state.

## Storage

Capacity per volume, not disk activity. Every mounted volume the OS lists,
including ones outside Caskade's library roots and removable drives (marked
"Removable"); the list follows drives being connected and removed.

- Pseudo filesystems are left out: `tmpfs`, `devtmpfs`, `overlay`, `squashfs`,
  `proc`, `sysfs`, `efivarfs` and the like, mounts under `/proc`, `/sys`,
  `/dev`, `/run/user`, `/snap`, Docker's and Podman's storage, AppImage mounts
  (`/tmp/.mount_*`) and macOS's `/System/Volumes/*`, and volumes of size 0.
  sysinfo already skips network filesystems (NFS, CIFS), which could hang.
- Each filesystem counts once: mounts of the same Linux block device (bind
  mounts, btrfs subvolumes) become one volume at its shortest mount point,
  listing the others. Without a device (Windows, macOS), volumes with the same
  label, capacity and free space are one (APFS lists the system volume twice).
- Names: the filesystem label (Linux reads `/dev/disk/by-label`), else
  "System" for `/`, "Local disk" for an unlabelled drive letter, else the
  mount point's last folder.

## Network

Receive and send rates in bytes per second (KiB/s, MiB/s), per interface and
in total. The total adds up physical interfaces only, so traffic that also
crosses a physical interface isn't counted twice. Virtual and loopback
interfaces are listed, marked "not counted".

- Linux: an interface is physical when `/sys/class/net/<name>/device` exists;
  `lo` is loopback; the rest (bridges, `veth`, VPNs, `tun`, Docker) is virtual.
- Windows and macOS: by name (`Loopback Pseudo-Interface`, `lo0`;
  `vEthernet`, VirtualBox, VMware, `utun`, `awdl`, `llw`, `bridge`, WireGuard,
  Tailscale, …) and by an all-zero hardware address, which tunnels have.

## Availability by OS

| Metric | Linux | Windows | macOS |
| --- | --- | --- | --- |
| CPU, memory | sysinfo | sysinfo | sysinfo |
| Storage | sysinfo, labels from `/dev/disk/by-label` | sysinfo | sysinfo |
| Network | sysinfo, kinds from sysfs | sysinfo, kinds by name | sysinfo, kinds by name |
| AMD GPUs | `gpu_busy_percent` and VRAM in `/sys/class/drm/card*/device` | unavailable | unavailable |
| Intel GPUs | listed; usage where the driver has `gpu_busy_percent` (i915 doesn't) | unavailable | unavailable |
| NVIDIA GPUs | `nvidia-smi`, matched to the DRM card by PCI address | `nvidia-smi` | — |

- NVIDIA: while sampling, Caskade runs one `nvidia-smi --query-gpu=… -lms 1000`
  (without a console window on Windows) and stops it with the sampler. Readings
  older than 3 s are a gap. Without nvidia-smi the GPU says to install it; when
  it fails or exits, its message shows and it is started again after 30 s.
  NVML through a library would need `unsafe` FFI, which the workspace forbids.
- Windows has no GPU source for AMD and Intel without WMI or PDH bindings, and
  macOS doesn't report GPU usage to apps; the GPU card says so instead of
  showing 0.

## Tests

`crates/caskade-core/src/system_stats/tests.rs` feeds the aggregator fake
readings (volumes, rates, gaps, classification), parses nvidia-smi lines,
reads a fake sysfs and drives the sampler with a fake probe (watch, lease,
history, a failing read). `views/settings/statistics/statistics.test.tsx`
renders the dashboard over `api/systemStatsFixtures.ts` (several GPUs, drives
and interfaces, nothing available, failed samples, a failed watch, stale data,
visibility). Storybook's `Shell/Statistics` shows each in desktop and console
modes.
