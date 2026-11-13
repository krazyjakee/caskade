//! The dashboard and its cards (the webview's `StatisticsDashboard.tsx`
//! and `Gauge.tsx`): CPU, memory, GPU, network and storage.

use super::*;

/// Where a graph sits in time.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Frame {
    pub(super) end: i64,
    pub(super) window_ms: i64,
    pub(super) interval_ms: i64,
}

#[component]
pub(super) fn StatisticsDashboard(stats: Stats, now: i64) -> Element {
    let status = stats.status(now);
    let latest = stats.samples.last().cloned();
    let frame = Frame {
        end: latest.as_ref().map_or(0, |latest| latest.at),
        window_ms: stats.history as i64 * stats.interval_ms,
        interval_ms: stats.interval_ms,
    };
    let gpus = stats
        .samples
        .iter()
        .rev()
        .find(|sample| sample.error.is_none())
        .map(|sample| sample.gpus.clone());
    let samples = stats.samples.clone();
    rsx! {
        section {
            "data-cs": "system-stats",
            "data-status": status.name(),
            "aria-labelledby": "system-stats-title",
            class: "group flex min-w-0 flex-col gap-5",
            header { class: "flex flex-col gap-1",
                h2 { id: "system-stats-title", "data-part": "title", class: "font-display text-2xl font-semibold tracking-wide",
                    {t!("Statistics")}
                }
                p { "data-part": "status", role: "status", class: "flex items-center gap-2 text-sm text-fg-muted",
                    span {
                        "aria-hidden": "true",
                        class: match status {
                            Status::Live => "size-2 shrink-0 rounded-full bg-status-installed",
                            Status::Error => "size-2 shrink-0 rounded-full bg-danger",
                            Status::Stale => "size-2 shrink-0 rounded-full bg-warning",
                            Status::Loading => "size-2 shrink-0 rounded-full bg-fg-subtle",
                        },
                    }
                    {status_text(&stats, status)}
                }
            }
            if !samples.is_empty() {
                div { class: "grid gap-4 lg:grid-cols-2",
                    CpuCard { samples: samples.clone(), frame }
                    MemoryCard { samples: samples.clone(), frame }
                    match gpus.as_ref().filter(|gpus| !gpus.devices.is_empty()) {
                        Some(gpus) => rsx! {
                            for gpu in gpus.devices.clone() {
                                GpuCard { key: "{gpu.id}", gpu, samples: samples.clone(), frame }
                            }
                        },
                        None => rsx! {
                            Card { metric: "gpu", title: t!(note = "The graphics card statistics card", "GPU"), icon: MONITOR_COG,
                                Unavailable {
                                    {gpus.and_then(|gpus| gpus.unavailable).or_else(|| latest.as_ref().and_then(|latest| latest.error.clone())).unwrap_or_else(|| t!("No GPU found."))}
                                }
                            }
                        },
                    }
                    NetworkCard { samples: samples.clone(), frame }
                    StorageCard { samples }
                }
            }
        }
    }
}

/// One metric: focusable, so a pad or the keyboard reaches every card.
#[component]
pub(super) fn Card(
    metric: &'static str,
    title: String,
    #[props(default)] detail: Option<String>,
    icon: Icon,
    #[props(default)] wide: bool,
    children: Element,
) -> Element {
    let heading = format!("stats-card-{metric}-{}", title.len());
    rsx! {
        article {
            "data-part": "card",
            "data-metric": metric,
            "aria-labelledby": "{heading}",
            tabindex: "0",
            "data-spatial": "focusable",
            class: if wide { "flex min-w-0 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-4 group-data-[status=stale]:opacity-70 sm:p-5 lg:col-span-2" } else { "flex min-w-0 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-4 group-data-[status=stale]:opacity-70 sm:p-5" },
            header { class: "flex min-w-0 items-center gap-2",
                Svg { icon, class: "size-4 shrink-0 text-accent-alt" }
                h3 { id: "{heading}", class: "truncate font-display text-lg font-semibold tracking-wide", "{title}" }
                if let Some(detail) = detail {
                    span { class: "ml-auto shrink-0 font-mono text-xs tracking-wider text-fg-muted uppercase", "{detail}" }
                }
            }
            {children}
        }
    }
}

#[component]
pub(super) fn Unavailable(children: Element) -> Element {
    rsx! {
        p { "data-part": "unavailable", class: "flex items-start gap-2 text-sm text-fg-muted",
            Svg { icon: CIRCLE_SLASH, class: "mt-0.5 size-4 shrink-0 text-fg-subtle" }
            span { {children} }
        }
    }
}

#[component]
pub(super) fn Facts(items: Vec<(String, String)>) -> Element {
    rsx! {
        dl { class: "grid min-w-0 flex-1 grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm",
            for (term, value) in items {
                dt { key: "{term}", class: "text-fg-muted", "{term}" }
                dd { class: "truncate text-right font-mono tabular-nums text-fg", "{value}" }
            }
        }
    }
}

/// A utilization gauge, 0–100 %, with its value written inside. `None`
/// draws an empty track.
#[component]
pub(super) fn Gauge(label: String, value: Option<f64>) -> Element {
    const RADIUS: f64 = 48.0;
    let circumference = 2.0 * std::f64::consts::PI * RADIUS;
    let arc = circumference * 0.75;
    let fraction = value.map_or(0.0, |value| (value / 100.0).clamp(0.0, 1.0));
    let high = value.is_some_and(|value| value >= HIGH);
    let text = match value {
        None => t!("No reading"),
        Some(value) if high => t!(
            note = "A gauge's reading at or above 90 %",
            "{percent}, high",
            percent = format::percent(Some(value))
        ),
        Some(value) => format::percent(Some(value)),
    };
    rsx! {
        div {
            "data-part": "gauge",
            role: "meter",
            "aria-label": "{label}",
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-valuenow": value.map(|value| value.round().to_string()),
            "aria-valuetext": "{text}",
            class: "relative size-28 shrink-0",
            svg { view_box: "0 0 120 120", "aria-hidden": "true", class: "size-full",
                circle {
                    cx: "60", cy: "60", r: "{RADIUS}", fill: "none", stroke: "currentColor",
                    stroke_width: "10", stroke_linecap: "round",
                    stroke_dasharray: "{arc} {circumference}",
                    transform: "rotate(135 60 60)",
                    class: "text-border",
                }
                if value.is_some() {
                    circle {
                        cx: "60", cy: "60", r: "{RADIUS}", fill: "none", stroke: "currentColor",
                        stroke_width: "10", stroke_linecap: "round",
                        stroke_dasharray: "{arc * fraction} {circumference}",
                        transform: "rotate(135 60 60)",
                        class: if high { "text-warning" } else { "text-accent-alt" },
                    }
                }
            }
            div { class: "absolute inset-0 flex flex-col items-center justify-center pt-1 text-center",
                span { class: if value.is_none() { "font-display text-2xl font-semibold tabular-nums text-fg-subtle" } else { "font-display text-2xl font-semibold tabular-nums text-fg" },
                    {format::percent(value)}
                }
                span { class: "font-mono text-[0.65rem] tracking-wider text-fg-muted uppercase",
                    match value {
                        None => t!(note = "A gauge without a reading", "n/a"),
                        Some(_) if high => tp!("gauge", "High"),
                        Some(_) => tp!("gauge", "Used"),
                    }
                }
            }
        }
    }
}

/// Why the newest value is missing.
pub(super) fn missing_reason(
    latest: Option<&SystemSample>,
    ever_read: bool,
    measuring: String,
) -> String {
    if let Some(error) = latest.and_then(|latest| latest.error.clone()) {
        return error;
    }
    if ever_read {
        t!("No reading this time.")
    } else {
        measuring
    }
}

pub(super) fn usage_label() -> String {
    tp!("statistics", "Usage")
}

#[component]
pub(super) fn CpuCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
    let latest = samples.last();
    let usage = latest
        .filter(|latest| latest.error.is_none())
        .and_then(|latest| latest.cpu.usage)
        .map(f64::from);
    let points = series(&samples, |sample| sample.cpu.usage.map(f64::from));
    let ever_read = points.iter().any(|point| point.1.is_some());
    let cores = samples
        .iter()
        .rev()
        .find(|sample| sample.cpu.cores > 0)
        .map(|sample| sample.cpu.cores);
    let label = t!("CPU usage");
    rsx! {
        Card {
            metric: "cpu",
            title: t!(note = "The processor's statistics card", "CPU"),
            icon: CPU,
            detail: cores.map(|cores| tn!("# thread", "# threads", u64::from(cores))),
            div { class: "flex items-center gap-4",
                Gauge { label: label.clone(), value: usage }
                div { class: "flex min-w-0 flex-1 flex-col gap-2",
                    Facts { items: vec![(usage_label(), format::percent(usage))] }
                    if usage.is_none() {
                        Unavailable { {missing_reason(latest, ever_read, t!("Measuring…"))} }
                    }
                }
            }
            Graph {
                label,
                series: vec![Series { name: usage_label(), points, dashed: false, tone: "text-accent-alt" }],
                frame,
                max: 100.0,
                rate_scale: false,
            }
        }
    }
}

#[component]
pub(super) fn MemoryCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
    let latest = samples.last();
    let memory = latest
        .filter(|latest| latest.error.is_none())
        .and_then(|latest| latest.memory.clone());
    let usage = memory
        .as_ref()
        .and_then(|memory| percent_of(memory.used, memory.total));
    let points = series(&samples, |sample| {
        sample
            .memory
            .as_ref()
            .and_then(|memory| percent_of(memory.used, memory.total))
    });
    let ever_read = points.iter().any(|point| point.1.is_some());
    let label = t!("Memory used");
    let used = tp!("memory", "Used");
    rsx! {
        Card {
            metric: "memory",
            title: t!(note = "The RAM statistics card", "Memory"),
            icon: MEMORY_STICK,
            detail: memory.as_ref().map(|memory| t!(note = "Installed memory", "{size} RAM", size = bytes(memory.total))),
            div { class: "flex items-center gap-4",
                Gauge { label: label.clone(), value: usage }
                match memory {
                    Some(memory) => rsx! {
                        Facts { items: vec![
                            (used.clone(), bytes(memory.used)),
                            (tp!("memory", "Available"), bytes(memory.available)),
                            (tp!("memory", "Total"), bytes(memory.total)),
                        ] }
                    },
                    None => rsx! {
                        Unavailable { {missing_reason(latest, ever_read, t!("Memory usage couldn't be read on this system."))} }
                    },
                }
            }
            Graph {
                label,
                series: vec![Series { name: used, points, dashed: false, tone: "text-accent" }],
                frame,
                max: 100.0,
                rate_scale: false,
            }
        }
    }
}

pub(super) fn vendor(vendor: GpuVendor) -> String {
    match vendor {
        GpuVendor::Amd => "AMD".to_owned(),
        GpuVendor::Intel => "Intel".to_owned(),
        GpuVendor::Nvidia => "NVIDIA".to_owned(),
        GpuVendor::Other => t!(note = "A graphics card from an unknown vendor", "GPU"),
    }
}

#[component]
pub(super) fn GpuCard(gpu: GpuSample, samples: Vec<SystemSample>, frame: Frame) -> Element {
    let latest = samples.last();
    let now = latest
        .filter(|latest| latest.error.is_none())
        .and_then(|latest| {
            latest
                .gpus
                .devices
                .iter()
                .find(|item| item.id == gpu.id)
                .cloned()
        });
    let usage = now.as_ref().and_then(|now| now.usage).map(f64::from);
    let points = series(&samples, |sample| {
        sample
            .gpus
            .devices
            .iter()
            .find(|item| item.id == gpu.id)
            .and_then(|item| item.usage)
            .map(f64::from)
    });
    let ever_read = points.iter().any(|point| point.1.is_some());
    let memory = match gpu.memory_total {
        Some(total) => t!(
            note = "Video memory used of the GPU's total",
            "{used} of {total}",
            used = gpu.memory_used.map_or_else(|| "—".to_owned(), bytes),
            total = bytes(total)
        ),
        None => "—".to_owned(),
    };
    let reason = match &now {
        Some(now) => now.unavailable.clone(),
        None => Some(missing_reason(latest, ever_read, t!("This GPU is gone."))),
    };
    let label = t!("{name} usage", name = gpu.name.clone());
    rsx! {
        Card {
            metric: "gpu",
            title: gpu.name.clone(),
            icon: MONITOR_COG,
            detail: format!("{} · {}", vendor(gpu.vendor), gpu.id),
            div { class: "flex items-center gap-4",
                Gauge { label: label.clone(), value: usage }
                div { class: "flex min-w-0 flex-1 flex-col gap-2",
                    Facts { items: vec![(usage_label(), format::percent(usage)), (t!("Video memory"), memory)] }
                    if let (None, Some(reason)) = (usage, reason) {
                        Unavailable { "{reason}" }
                    }
                }
            }
            if ever_read {
                Graph {
                    label,
                    series: vec![Series { name: usage_label(), points, dashed: false, tone: "text-accent-alt" }],
                    frame,
                    max: 100.0,
                    rate_scale: false,
                }
            }
        }
    }
}

pub(super) fn kind_label(kind: InterfaceKind) -> String {
    match kind {
        InterfaceKind::Physical => t!(
            note = "A network interface counted in the totals",
            "Counted"
        ),
        InterfaceKind::Virtual => t!("Virtual, not counted"),
        InterfaceKind::Loopback => t!("Loopback, not counted"),
    }
}

pub(super) fn kind_name(kind: InterfaceKind) -> &'static str {
    match kind {
        InterfaceKind::Physical => "physical",
        InterfaceKind::Virtual => "virtual",
        InterfaceKind::Loopback => "loopback",
    }
}

#[component]
pub(super) fn Swatch(dashed: bool) -> Element {
    rsx! {
        svg { "aria-hidden": "true", view_box: "0 0 16 4", class: "h-1 w-4 shrink-0 overflow-visible",
            line { x1: "0", x2: "16", y1: "2", y2: "2", stroke: "currentColor", stroke_width: "2", stroke_dasharray: dashed.then_some("4 3") }
        }
    }
}

#[component]
pub(super) fn NetworkCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
    let latest = samples.last();
    let good = samples.iter().rev().find(|sample| sample.error.is_none());
    let network = latest
        .filter(|latest| latest.error.is_none())
        .map(|latest| latest.network.clone());
    let received = series(&samples, |sample| sample.network.received);
    let sent = series(&samples, |sample| sample.network.transmitted);
    let peak = received
        .iter()
        .chain(sent.iter())
        .filter_map(|point| point.1)
        .fold(0.0, f64::max);
    let interfaces = good
        .map(|good| good.network.interfaces.clone())
        .unwrap_or_default();
    let receiving = tp!("network", "Receiving");
    let sending = tp!("network", "Sending");
    rsx! {
        Card { metric: "network", title: t!(note = "The network statistics card", "Network"), icon: NETWORK, wide: true,
            if interfaces.is_empty() {
                Unavailable { {latest.and_then(|latest| latest.error.clone()).unwrap_or_else(|| t!("No network interfaces found."))} }
            } else {
                div { class: "flex flex-wrap gap-x-8 gap-y-2",
                    p { class: "flex items-center gap-2 text-accent-alt",
                        Swatch { dashed: false }
                        span { class: "text-sm text-fg-muted", "↓ {receiving}" }
                        span { class: "font-display text-xl font-semibold tabular-nums text-fg", {rate(network.as_ref().and_then(|network| network.received))} }
                    }
                    p { class: "flex items-center gap-2 text-accent",
                        Swatch { dashed: true }
                        span { class: "text-sm text-fg-muted", "↑ {sending}" }
                        span { class: "font-display text-xl font-semibold tabular-nums text-fg", {rate(network.as_ref().and_then(|network| network.transmitted))} }
                    }
                }
                Graph {
                    label: t!("Network traffic"),
                    series: vec![
                        Series { name: receiving.clone(), points: received, dashed: false, tone: "text-accent-alt" },
                        Series { name: sending.clone(), points: sent, dashed: true, tone: "text-accent" },
                    ],
                    frame,
                    max: nice_ceiling(peak),
                    rate_scale: true,
                }
                ul { class: "flex flex-col divide-y divide-border text-sm",
                    for item in interfaces {
                        li {
                            key: "{item.name}",
                            "data-part": "interface",
                            "data-kind": kind_name(item.kind),
                            class: if item.kind == InterfaceKind::Physical { "flex flex-wrap items-baseline gap-x-4 gap-y-1 py-1.5" } else { "flex flex-wrap items-baseline gap-x-4 gap-y-1 py-1.5 text-fg-muted" },
                            span { class: "min-w-0 truncate font-mono", "{item.name}" }
                            span { class: "text-xs text-fg-muted", {kind_label(item.kind)} }
                            span { class: "ml-auto font-mono tabular-nums", "↓ {rate(item.received)} · ↑ {rate(item.transmitted)}" }
                        }
                    }
                }
                p { class: "text-xs text-fg-muted",
                    {t!("Totals add up physical interfaces only, so traffic through a VPN, a container or loopback isn't counted twice. — means no rate yet.")}
                }
            }
        }
    }
}

#[component]
pub(super) fn Volume(disk: DiskSample) -> Element {
    let used = disk.total.saturating_sub(disk.available);
    let share = percent_of(used, disk.total).unwrap_or(0.0);
    let full = share >= HIGH;
    let mounts = std::iter::once(disk.mount.clone())
        .chain(disk.other_mounts.iter().cloned())
        .collect::<Vec<_>>()
        .join(", ");
    let fill = if full {
        "h-full rounded-full bg-warning"
    } else {
        "h-full rounded-full bg-accent-alt"
    };
    let file_system = if disk.file_system.is_empty() {
        String::new()
    } else {
        format!(" · {}", disk.file_system)
    };
    rsx! {
        li { "data-part": "volume", class: "flex min-w-0 flex-col gap-1.5",
            div { class: "flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5",
                span { class: "font-medium", "{disk.name}" }
                span { class: "min-w-0 truncate font-mono text-xs text-fg-muted", title: "{mounts}", "{mounts}" }
                if disk.removable {
                    span { class: "rounded-control border border-border px-1.5 text-xs text-fg-muted",
                        {t!(note = "A removable drive", "Removable")}
                    }
                }
                span { class: "ml-auto font-mono text-xs tabular-nums",
                    if full {
                        {t!(note = "A drive at or above 90 % full", "{percent} full · almost out of space", percent = format::percent(Some(share)))}
                    } else {
                        {t!(note = "A drive's use", "{percent} full", percent = format::percent(Some(share)))}
                    }
                }
            }
            div {
                "data-part": "capacity",
                role: "meter",
                "aria-label": t!("{name} ({mount}) space used", name = disk.name.clone(), mount = disk.mount.clone()),
                "aria-valuemin": "0",
                "aria-valuemax": "100",
                "aria-valuenow": "{share.round()}",
                "aria-valuetext": t!("{used} used of {total}, {free} free", used = bytes(used), total = bytes(disk.total), free = bytes(disk.available)),
                class: "h-2.5 overflow-hidden rounded-full bg-surface-base/70",
                div { class: fill, style: "width: {share}%" }
            }
            div { class: "flex flex-wrap justify-between gap-x-4 font-mono text-xs text-fg-muted tabular-nums",
                span { {t!("{used} used", used = bytes(used))} }
                span {
                    {t!("{free} free of {total}", free = bytes(disk.available), total = bytes(disk.total))}
                    "{file_system}"
                }
            }
        }
    }
}

#[component]
pub(super) fn StorageCard(samples: Vec<SystemSample>) -> Element {
    // Capacity changes slowly: a failed sample keeps the last volumes.
    let disks = samples
        .iter()
        .rev()
        .find(|sample| sample.error.is_none())
        .map(|sample| sample.disks.clone())
        .unwrap_or_default();
    rsx! {
        Card {
            metric: "storage",
            title: t!(note = "The drives statistics card", "Storage"),
            icon: HARD_DRIVE,
            detail: t!(note = "The storage card shows capacity, not speed", "Capacity"),
            wide: true,
            if disks.is_empty() {
                Unavailable { {t!("No drives found.")} }
            } else {
                ul { class: "flex flex-col gap-4",
                    for disk in disks {
                        Volume { key: "{disk.id}", disk }
                    }
                }
            }
        }
    }
}
