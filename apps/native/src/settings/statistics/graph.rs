//! The rate graphs (the webview's `Graph.tsx`): series, their lines and
//! areas, and a round top.

use super::*;

/// `part` of `whole` in percent; `None` without a whole.
pub(super) fn percent_of(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| (part as f64 / whole as f64 * 100.0).clamp(0.0, 100.0))
}

/// A round top for a rate graph: 1, 2 or 5 × a power of ten.
pub(super) fn nice_ceiling(value: f64) -> f64 {
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

pub(super) const WIDTH: f64 = 240.0;

pub(super) const HEIGHT: f64 = 64.0;

/// One line of a graph.
#[derive(Clone, PartialEq)]
pub(super) struct Series {
    /// Its name in the summary ("Receiving").
    pub(super) name: String,
    pub(super) points: Vec<(i64, Option<f64>)>,
    /// A dashed line, so two series differ by more than colour.
    pub(super) dashed: bool,
    /// A token colour utility for the line.
    pub(super) tone: &'static str,
}

/// The line's runs of points: a missing value or a jump of more than `gap`
/// between samples breaks it.
pub(super) fn runs(
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

pub(super) fn line(run: &[(f64, f64)]) -> String {
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

pub(super) fn area(run: &[(f64, f64)]) -> String {
    let first = run.first().map_or(0.0, |point| point.0);
    let last = run.last().map_or(0.0, |point| point.0);
    format!("{} L{last:.2},{HEIGHT} L{first:.2},{HEIGHT} Z", line(run))
}

/// A recent-history graph: time runs left to right over the frame's
/// window. The values are written beside it by the card; this names its
/// range and summarises it for assistive technology.
#[component]
pub(super) fn Graph(
    label: String,
    series: Vec<Series>,
    frame: Frame,
    max: f64,
    rate_scale: bool,
) -> Element {
    let format_value = move |value: f64| {
        if rate_scale {
            rate(Some(value))
        } else {
            format::percent(Some(value))
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

pub(super) fn series(
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
