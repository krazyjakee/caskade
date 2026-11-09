//! Caskade spike: headless Blitz probe.
//!
//!   caskade_probe static <snapshot.html> <out.png>
//!       Render a script-free snapshot of Caskade's UI (DOM + inlined CSS).
//!   caskade_probe script <ui-dist dir> <route> <out.png> [ms]
//!       Run Caskade's real Vite bundle in Blitz's Boa runtime, serving
//!       module requests for http://localhost:1533/* from <ui-dist dir>.
//!
//! Prints a timing and the process's RSS after each stage.

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{Document, DocumentConfig, util::Color};
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use blitz_vibey_script::{FetchError, ScriptDocument, ScriptFetcher};
use peniko::Fill;
use peniko::kurbo::Rect;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const W: u32 = 1400;
const H: u32 = 900;

fn rss() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .unwrap_or(0)
    };
    format!(
        "rss={}MB anon={}MB threads={}",
        field("VmRSS:") / 1024,
        field("RssAnon:") / 1024,
        field("Threads:")
    )
}

struct Timer(Instant, Instant);
impl Timer {
    fn new() -> Self {
        let t = Instant::now();
        Self(t, t)
    }
    fn mark(&mut self, what: &str) {
        let now = Instant::now();
        println!(
            "{what:<34} {:>7.1} ms  (total {:>7.1} ms)  {}",
            (now - self.1).as_secs_f64() * 1e3,
            (now - self.0).as_secs_f64() * 1e3,
            rss()
        );
        self.1 = now;
    }
}

/// Serves the app's origin from the built bundle on disk.
struct DistFetcher(PathBuf);
impl ScriptFetcher for DistFetcher {
    fn fetch(&self, url: &url::Url) -> Result<String, FetchError> {
        let path = url.path().trim_start_matches('/');
        let file = self.0.join(if path.is_empty() || !path.contains('.') {
            "index.html"
        } else {
            path
        });
        std::fs::read_to_string(&file).map_err(FetchError::Io)
    }
}

fn viewport() -> Viewport {
    Viewport::new(W, H, 1.0, ColorScheme::Dark)
}

fn anim_time() -> f64 {
    std::env::var("PROBE_TIME").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

fn settle(doc: &mut dyn Document, net: &Provider) {
    loop {
        doc.inner_mut().resolve(anim_time());
        if net.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn paint(doc: &mut dyn Document, out: &str) {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::BLACK,
                Default::default(),
                &Rect::new(0.0, 0.0, W as f64, H as f64),
            );
            paint_scene(scene, &mut *doc.inner_mut(), 1.0, W, H, 0, 0);
        },
        W,
        H,
    );
    let file = std::fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(file, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&buffer).unwrap();
}

fn count_nodes(doc: &dyn Document) -> usize {
    doc.inner().query_selector_all("*").map(|v| v.len()).unwrap_or(0)
}

/// Re-resolve with nothing changed, then after flipping an attribute the
/// whole stylesheet keys off (`data-color-scheme` on <html>), as a theme change would.
fn restyle_costs(doc: &mut dyn Document, t: &mut Timer) {
    doc.inner_mut().resolve(0.0);
    t.mark("resolve again, nothing changed");
    let root = doc.inner().query_selector("html").unwrap().unwrap();
    {
        let mut inner = doc.inner_mut();
        let mut m = inner.mutate();
        m.set_attribute(
            root,
            blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from("data-density")),
            "compact",
        );
    }
    doc.inner_mut().resolve(0.0);
    t.mark("resolve after <html> attr change");
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut t = Timer::new();
    t.mark("start");
    let net = Arc::new(Provider::new(None));

    match args.get(1).map(String::as_str) {
        Some("static") => {
            let mut html = std::fs::read_to_string(&args[2]).unwrap();
            if let Ok(css) = std::env::var("PROBE_CSS") {
                html = html.replace("</head>", &format!("<style>{css}</style></head>"));
            }
            let mut doc = HtmlDocument::from_html(
                &html,
                DocumentConfig {
                    base_url: Some("http://localhost:1533/".into()),
                    net_provider: Some(Arc::clone(&net) as _),
                    viewport: Some(viewport()),
                    ..Default::default()
                },
            );
            t.mark("parse HTML + CSS");
            settle(&mut doc, &net);
            t.mark("fetch fonts/images + resolve");
            println!("nodes: {}", count_nodes(&doc));
            paint(&mut doc, &args[3]);
            t.mark("paint (vello_cpu) + png");
            restyle_costs(&mut doc, &mut t);
        }
        Some("script") => {
            let dist = PathBuf::from(&args[2]);
            let route = &args[3];
            let wait_ms: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(3000);
            let html = std::fs::read_to_string(dist.join("index.html")).unwrap();
            let mut doc = ScriptDocument::from_html(
                &html,
                DocumentConfig {
                    base_url: Some(format!("http://localhost:1533{route}")),
                    net_provider: Some(Arc::clone(&net) as _),
                    viewport: Some(viewport()),
                    ..Default::default()
                },
            )
            .without_timer_thread()
            .with_fetcher(DistFetcher(dist));
            t.mark("parse index.html");
            if let Ok(js) = std::env::var("PROBE_PRELUDE") {
                doc.eval(&std::fs::read_to_string(js).unwrap());
                t.mark("prelude");
            }
            doc.execute_scripts();
            t.mark("execute module scripts");
            let deadline = Instant::now() + Duration::from_millis(wait_ms);
            let mut polls = 0;
            while Instant::now() < deadline {
                doc.poll(None);
                doc.inner_mut().resolve(0.0);
                polls += 1;
                std::thread::sleep(Duration::from_millis(16));
            }
            t.mark(&format!("run timers/frames ({polls} polls)"));
            doc.eval("{const d=document.createElement('pre');d.id='probe-log';d.textContent=(globalThis.__log||[]).join('\\n');document.body.appendChild(d);}");
            if let Some(id) = doc.inner().query_selector("#probe-log").unwrap() {
                let text = doc.inner().get_node(id).unwrap().text_content();
                for line in text.lines().take(60) {
                    println!("log: {}", line.chars().take(400).collect::<String>());
                }
            }
            settle(&mut doc, &net);
            t.mark("fetch css/fonts + resolve");
            for m in doc.take_messages().iter().take(40) {
                println!("console: {}", m.chars().take(300).collect::<String>());
            }
            for e in doc.take_js_errors().iter().take(40) {
                println!("js error: {}", e.chars().take(600).collect::<String>());
            }
            println!("nodes: {}", count_nodes(&doc));
            paint(&mut doc, &args[4]);
            t.mark("paint (vello_cpu) + png");
        }
        _ => eprintln!("usage: see file header"),
    }
}
