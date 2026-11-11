//! Settings → Statistics (#156, #105), as the webview's `statistics/`:
//! this machine's CPU, GPUs, memory, network and storage, live. It watches
//! the core's sampler only while it is shown and the window is visible
//! (`useSystemStats`): a watch renewed every ten seconds, each sample as it
//! comes, staleness checked between samples.

use std::time::Duration;

use caskade_system_stats::{
    DiskSample, GpuSample, GpuVendor, InterfaceKind, SystemSample, SystemStats, SystemStatsSnapshot,
};
use caskade_ui_i18n::format;
use caskade_ui_i18n::intl::Clock;
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::prelude::*;

use crate::ui::icons::{CIRCLE_SLASH, CPU, HARD_DRIVE, MEMORY_STICK, MONITOR_COG, NETWORK};
use crate::ui::{Icon, Svg};

/// How often a watch is renewed; the core lets one lapse after 30 s.
const RENEW: Duration = Duration::from_secs(10);
/// Samples older than this many intervals make the view stale.
const STALE_INTERVALS: i64 = 3;
/// At or above this a gauge says "High" and turns to the warning colour.
const HIGH: f64 = 90.0;

/// The app's sampler: a test's, else one for the app, made the first time.
pub fn use_system_stats() -> SystemStats {
    try_use_context::<SystemStats>().unwrap_or_else(|| {
        static APP: std::sync::OnceLock<SystemStats> = std::sync::OnceLock::new();
        APP.get_or_init(SystemStats::new).clone()
    })
}

async fn sleep(time: Duration) {
    // Timers need the app runtime's reactor; Dioxus's executor has none.
    let _ = caskade_app::runtime::spawn(async move { tokio::time::sleep(time).await }).await;
}

/// The main window is shown (not hidden to the tray or minimized); tests
/// have none and count as shown.
fn window_shown() -> bool {
    match crate::shell::main_window() {
        Some(window) => window.is_visible() != Some(false) && window.is_minimized() != Some(true),
        None => true,
    }
}

/// What the dashboard shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Loading,
    Live,
    Stale,
    Error,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Live => "live",
            Self::Stale => "stale",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, PartialEq)]
struct Stats {
    /// Oldest first, at most `history`.
    samples: Vec<SystemSample>,
    interval_ms: i64,
    history: usize,
    /// Why the last watch failed.
    error: Option<String>,
    /// When the last sample came, Unix milliseconds.
    received_at: Option<i64>,
}

impl Stats {
    /// Adds `incoming` once each (by `seq`), oldest first, keeping
    /// `history`.
    fn merge(&mut self, incoming: Vec<SystemSample>) {
        for sample in incoming {
            match self
                .samples
                .iter_mut()
                .find(|known| known.seq == sample.seq)
            {
                Some(known) => *known = sample,
                None => self.samples.push(sample),
            }
        }
        self.samples.sort_by_key(|sample| sample.seq);
        let keep = self.history.max(1);
        if self.samples.len() > keep {
            self.samples.drain(..self.samples.len() - keep);
        }
    }

    fn status(&self, now: i64) -> Status {
        if self.samples.is_empty() {
            return if self.error.is_some() {
                Status::Error
            } else {
                Status::Loading
            };
        }
        match self.received_at {
            Some(at) if now - at <= self.interval_ms * STALE_INTERVALS => {
                if self.error.is_some() {
                    Status::Stale
                } else {
                    Status::Live
                }
            }
            _ => Status::Stale,
        }
    }
}

fn now_ms() -> i64 {
    caskade_core::now_ms()
}

/// Settings → Statistics.
#[component]
pub fn StatisticsSettings() -> Element {
    let stats = use_system_stats();
    let mut state = use_signal(|| Stats {
        samples: Vec::new(),
        interval_ms: 1000,
        history: 60,
        error: None,
        received_at: None,
    });
    let mut now = use_signal(now_ms);
    let watcher = use_hook(|| {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        format!("native-stats-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    });
    {
        let (stats, watcher) = (stats.clone(), watcher.clone());
        use_hook(move || {
            // Each sample as it comes.
            let mut samples = stats.subscribe();
            spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match samples.recv().await {
                        Ok(sample) => {
                            let mut current = state.write();
                            current.merge(vec![sample]);
                            current.received_at = Some(now_ms());
                        }
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            // The watch, renewed while shown; stopped while the window is
            // hidden or minimized.
            spawn(async move {
                let mut watching = false;
                let mut fresh = true;
                let mut since_renew = Duration::ZERO;
                loop {
                    let shown = window_shown();
                    if shown && (!watching || since_renew >= RENEW) {
                        let history = fresh;
                        let (stats, watcher) = (stats.clone(), watcher.clone());
                        let answer =
                            caskade_app::runtime::spawn(
                                async move { stats.watch(&watcher, history) },
                            )
                            .await;
                        match answer {
                            Ok(SystemStatsSnapshot {
                                interval_ms,
                                history,
                                samples,
                            }) => {
                                fresh = false;
                                watching = true;
                                let mut current = state.write();
                                current.interval_ms = i64::from(interval_ms);
                                current.history = history as usize;
                                current.error = None;
                                if let Some(newest) = samples.last() {
                                    // A history is as fresh as its newest sample.
                                    current.received_at.get_or_insert(newest.at.min(now_ms()));
                                }
                                current.merge(samples);
                            }
                            Err(error) => {
                                fresh = true;
                                state.write().error = Some(error.to_string());
                            }
                        }
                        since_renew = Duration::ZERO;
                    } else if !shown && watching {
                        watching = false;
                        fresh = true;
                        stats.unwatch(&watcher);
                    }
                    let tick = Duration::from_millis(state.peek().interval_ms.max(250) as u64);
                    sleep(tick).await;
                    since_renew += tick;
                    // Re-checks staleness between samples.
                    now.set(now_ms());
                }
            });
        });
    }
    {
        let (stats, watcher) = (stats.clone(), watcher.clone());
        use_drop(move || stats.unwatch(&watcher));
    }
    let current = state.read().clone();
    rsx! {
        StatisticsDashboard { stats: current, now: now() }
    }
}

/// `60 s`, `2 min`.
fn window_label(ms: i64) -> String {
    let seconds = (ms as f64 / 1000.0).round() as i64;
    let minutes = (seconds as f64 / 60.0).round() as i64;
    if seconds < 120 {
        t!(
            note = "A span of time in seconds, abbreviated",
            "{seconds} s",
            seconds = seconds
        )
    } else {
        t!(
            note = "A span of time in minutes, abbreviated",
            "{minutes} min",
            minutes = minutes
        )
    }
}

fn percent(value: Option<f64>) -> String {
    format::percent(value)
}

fn bytes(value: u64) -> String {
    format::bytes(Some(value))
}

fn rate(bytes_per_second: Option<f64>) -> String {
    match bytes_per_second {
        None => "—".to_owned(),
        Some(rate) => t!(
            note = "A rate: bytes per second",
            "{size}/s",
            size = format::bytes(Some(rate.max(0.0) as u64))
        ),
    }
}

/// `part` of `whole` in percent; `None` without a whole.
fn percent_of(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| (part as f64 / whole as f64 * 100.0).clamp(0.0, 100.0))
}

/// A round top for a rate graph: 1, 2 or 5 × a power of ten.
fn nice_ceiling(value: f64) -> f64 {
    if value <= 0.0 {
        return 1024.0;
    }
    let magnitude = 10f64.powf(value.log10().floor());
    for step in [1.0, 2.0, 5.0, 10.0] {
        if value <= step * magnitude {
            return step * magnitude;
        }
    }
    10.0 * magnitude
}

/// `error` as the end of a sentence.
fn clause(error: &str) -> String {
    error.trim().trim_end_matches('.').to_owned()
}

fn status_text(stats: &Stats, status: Status) -> String {
    let shown = window_label(stats.history as i64 * stats.interval_ms);
    match status {
        Status::Loading => t!("Waiting for the first reading…"),
        Status::Live if stats.interval_ms == 1000 => t!(
            "Live: updates every second, graphs show the last {span}.",
            span = shown
        ),
        Status::Live => t!(
            "Live: updates every {interval}, graphs show the last {span}.",
            interval = window_label(stats.interval_ms),
            span = shown
        ),
        Status::Stale => {
            let when = match stats.samples.last() {
                Some(latest) => format::time(
                    crate::library::local_date_time(latest.at),
                    Clock::System,
                    true,
                ),
                None => t!("a while ago"),
            };
            match &stats.error {
                None => t!(
                    "Not updating: the last reading is from {when}. Trying again.",
                    when = when
                ),
                Some(error) => t!(
                    note = "{reason} is the sampler's error",
                    "Not updating: the last reading is from {when}. {reason}. Trying again.",
                    when = when,
                    reason = clause(error)
                ),
            }
        }
        Status::Error => {
            let reason = clause(
                &stats
                    .error
                    .clone()
                    .unwrap_or_else(|| t!("the sampler didn't answer")),
            );
            t!(
                "Statistics aren't available: {reason}. Trying again.",
                reason = reason
            )
        }
    }
}

/// Where a graph sits in time.
#[derive(Clone, Copy, PartialEq)]
struct Frame {
    end: i64,
    window_ms: i64,
    interval_ms: i64,
}

#[component]
fn StatisticsDashboard(stats: Stats, now: i64) -> Element {
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
fn Card(
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
fn Unavailable(children: Element) -> Element {
    rsx! {
        p { "data-part": "unavailable", class: "flex items-start gap-2 text-sm text-fg-muted",
            Svg { icon: CIRCLE_SLASH, class: "mt-0.5 size-4 shrink-0 text-fg-subtle" }
            span { {children} }
        }
    }
}

#[component]
fn Facts(items: Vec<(String, String)>) -> Element {
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
fn Gauge(label: String, value: Option<f64>) -> Element {
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
            percent = percent(Some(value))
        ),
        Some(value) => percent(Some(value)),
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
                    {percent(value)}
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

const WIDTH: f64 = 240.0;
const HEIGHT: f64 = 64.0;

/// One line of a graph.
#[derive(Clone, PartialEq)]
struct Series {
    /// Its name in the summary ("Receiving").
    name: String,
    points: Vec<(i64, Option<f64>)>,
    /// A dashed line, so two series differ by more than colour.
    dashed: bool,
    /// A token colour utility for the line.
    tone: &'static str,
}

/// The line's runs of points: a missing value or a jump of more than `gap`
/// between samples breaks it.
fn runs(
    points: &[(i64, Option<f64>)],
    x: impl Fn(i64) -> f64,
    y: impl Fn(f64) -> f64,
    gap: i64,
) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut last: Option<i64> = None;
    for &(at, value) in points {
        let broken = value.is_none() || last.is_some_and(|last| at - last > gap);
        if broken && !run.is_empty() {
            out.push(std::mem::take(&mut run));
        }
        match value {
            None => last = None,
            Some(value) => {
                run.push((x(at), y(value)));
                last = Some(at);
            }
        }
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

fn line(run: &[(f64, f64)]) -> String {
    // A lone point still shows, as a dot from its round cap.
    let points: Vec<(f64, f64)> = match run {
        [only] => vec![*only, (only.0 + 0.01, only.1)],
        _ => run.to_vec(),
    };
    points
        .iter()
        .enumerate()
        .map(|(i, (px, py))| format!("{}{px:.2},{py:.2}", if i == 0 { "M" } else { "L" }))
        .collect::<Vec<_>>()
        .join(" ")
}

fn area(run: &[(f64, f64)]) -> String {
    let first = run.first().map_or(0.0, |point| point.0);
    let last = run.last().map_or(0.0, |point| point.0);
    format!("{} L{last:.2},{HEIGHT} L{first:.2},{HEIGHT} Z", line(run))
}

/// A recent-history graph: time runs left to right over the frame's
/// window. The values are written beside it by the card; this names its
/// range and summarises it for assistive technology.
#[component]
fn Graph(label: String, series: Vec<Series>, frame: Frame, max: f64, rate_scale: bool) -> Element {
    let format_value = move |value: f64| {
        if rate_scale {
            rate(Some(value))
        } else {
            percent(Some(value))
        }
    };
    let start = frame.end - frame.window_ms;
    let window = frame.window_ms.max(1) as f64;
    let x = move |at: i64| (at - start) as f64 / window * WIDTH;
    let y = move |value: f64| HEIGHT - (value.clamp(0.0, max) / max) * (HEIGHT - 2.0) - 1.0;
    let summary = series
        .iter()
        .map(|item| {
            let values: Vec<f64> = item.points.iter().filter_map(|point| point.1).collect();
            if values.is_empty() {
                return t!("{name}: no readings", name = item.name.clone());
            }
            let now = match item.points.last().and_then(|point| point.1) {
                Some(last) => format_value(last),
                None => t!("no reading"),
            };
            let peak = format_value(values.iter().cloned().fold(f64::MIN, f64::max));
            let low = format_value(values.iter().cloned().fold(f64::MAX, f64::min));
            let missed = item.points.len() - values.len();
            if missed > 0 {
                t!(
                    note = "A graph's summary; {missed} is how many samples couldn't be read",
                    "{name}: now {now}, peak {peak}, low {low}, {missed} missed",
                    name = item.name.clone(),
                    now = now,
                    peak = peak,
                    low = low,
                    missed = missed
                )
            } else {
                t!(
                    "{name}: now {now}, peak {peak}, low {low}",
                    name = item.name.clone(),
                    now = now,
                    peak = peak,
                    low = low
                )
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    let described = t!(
        note = "A graph: what it shows, the time it covers and its summary",
        "{label}, last {span}. {summary}.",
        label = label,
        span = window_label(frame.window_ms),
        summary = summary
    );
    let gap = (frame.interval_ms as f64 * 2.5) as i64;
    rsx! {
        figure { "data-part": "graph", class: "flex min-w-0 flex-col gap-1",
            div { class: "relative",
                svg {
                    view_box: "0 0 {WIDTH} {HEIGHT}",
                    preserve_aspect_ratio: "none",
                    role: "img",
                    "aria-label": "{described}",
                    class: "h-16 w-full overflow-visible",
                    for level in [0.25, 0.5, 0.75] {
                        line {
                            x1: "0", x2: "{WIDTH}", y1: "{HEIGHT * level}", y2: "{HEIGHT * level}",
                            stroke: "currentColor", stroke_width: "1",
                            class: "text-border opacity-60",
                        }
                    }
                    for (index, item) in series.iter().enumerate() {
                        for run in runs(&item.points, x, y, gap) {
                            g { class: item.tone,
                                if index == 0 {
                                    path { d: "{area(&run)}", fill: "currentColor", fill_opacity: "0.12", stroke: "none" }
                                }
                                path {
                                    d: "{line(&run)}",
                                    fill: "none",
                                    stroke: "currentColor",
                                    stroke_width: "2",
                                    stroke_linejoin: "round",
                                    stroke_linecap: "round",
                                    stroke_dasharray: item.dashed.then_some("5 4"),
                                }
                            }
                        }
                    }
                }
                span { class: "pointer-events-none absolute top-0 left-1 font-mono text-[0.65rem] text-fg-subtle",
                    {format_value(max)}
                }
            }
            div { "aria-hidden": "true", class: "flex justify-between font-mono text-[0.65rem] tracking-wider text-fg-subtle uppercase",
                span { "−{window_label(frame.window_ms)}" }
                span { {t!(note = "The right end of a history graph's time axis", "now")} }
            }
        }
    }
}

fn series(
    samples: &[SystemSample],
    pick: impl Fn(&SystemSample) -> Option<f64>,
) -> Vec<(i64, Option<f64>)> {
    samples
        .iter()
        .map(|sample| {
            (
                sample.at,
                if sample.error.is_some() {
                    None
                } else {
                    pick(sample)
                },
            )
        })
        .collect()
}

/// Why the newest value is missing.
fn missing_reason(latest: Option<&SystemSample>, ever_read: bool, measuring: String) -> String {
    if let Some(error) = latest.and_then(|latest| latest.error.clone()) {
        return error;
    }
    if ever_read {
        t!("No reading this time.")
    } else {
        measuring
    }
}

fn usage_label() -> String {
    tp!("statistics", "Usage")
}

#[component]
fn CpuCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
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
                    Facts { items: vec![(usage_label(), percent(usage))] }
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
fn MemoryCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
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

fn vendor(vendor: GpuVendor) -> String {
    match vendor {
        GpuVendor::Amd => "AMD".to_owned(),
        GpuVendor::Intel => "Intel".to_owned(),
        GpuVendor::Nvidia => "NVIDIA".to_owned(),
        GpuVendor::Other => t!(note = "A graphics card from an unknown vendor", "GPU"),
    }
}

#[component]
fn GpuCard(gpu: GpuSample, samples: Vec<SystemSample>, frame: Frame) -> Element {
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
                    Facts { items: vec![(usage_label(), percent(usage)), (t!("Video memory"), memory)] }
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

fn kind_label(kind: InterfaceKind) -> String {
    match kind {
        InterfaceKind::Physical => t!(
            note = "A network interface counted in the totals",
            "Counted"
        ),
        InterfaceKind::Virtual => t!("Virtual, not counted"),
        InterfaceKind::Loopback => t!("Loopback, not counted"),
    }
}

fn kind_name(kind: InterfaceKind) -> &'static str {
    match kind {
        InterfaceKind::Physical => "physical",
        InterfaceKind::Virtual => "virtual",
        InterfaceKind::Loopback => "loopback",
    }
}

#[component]
fn Swatch(dashed: bool) -> Element {
    rsx! {
        svg { "aria-hidden": "true", view_box: "0 0 16 4", class: "h-1 w-4 shrink-0 overflow-visible",
            line { x1: "0", x2: "16", y1: "2", y2: "2", stroke: "currentColor", stroke_width: "2", stroke_dasharray: dashed.then_some("4 3") }
        }
    }
}

#[component]
fn NetworkCard(samples: Vec<SystemSample>, frame: Frame) -> Element {
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
fn Volume(disk: DiskSample) -> Element {
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
                        {t!(note = "A drive at or above 90 % full", "{percent} full · almost out of space", percent = percent(Some(share)))}
                    } else {
                        {t!(note = "A drive's use", "{percent} full", percent = percent(Some(share)))}
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
fn StorageCard(samples: Vec<SystemSample>) -> Element {
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

#[cfg(test)]
mod tests {
    use super::{nice_ceiling, percent_of, runs};

    #[test]
    fn rate_graphs_top_out_at_a_round_number() {
        assert_eq!(nice_ceiling(0.0), 1024.0);
        assert_eq!(nice_ceiling(3_000.0), 5_000.0);
        assert_eq!(nice_ceiling(12_000.0), 20_000.0);
        assert_eq!(percent_of(5, 0), None);
        assert_eq!(percent_of(50, 200), Some(25.0));
    }

    #[test]
    fn a_gap_or_a_missing_value_breaks_the_line() {
        let points = [
            (0, Some(1.0)),
            (1000, Some(2.0)),
            (2000, None),
            (3000, Some(3.0)),
            (9000, Some(4.0)),
        ];
        let lines = runs(&points, |at| at as f64, |value| value, 2500);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), 2);
    }
}
