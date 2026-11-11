//! Settings → About (#162, #105), as the webview's `about/`: the app and
//! its build first (#68), the data services Caskade credits, and the way
//! into Legal, where `?section=` opens Caskade's licences, the open-source
//! packages it ships, its terms or its privacy policy in place, bundled so
//! they read offline. The documents stay in English as published; only
//! their labels are translated.

use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::prelude::keyboard_types::NamedKey;
use dioxus_native::prelude::*;
use serde::Deserialize;

use crate::router::use_router;
use crate::ui::icons::{
    ARROW_LEFT, BUG, CHEVRON_DOWN, CHEVRON_RIGHT, CIRCLE_ARROW_UP, EXTERNAL_LINK, FILE_TEXT,
    GIT_BRANCH, PACKAGE_OPEN, SCALE, SHIELD_CHECK, TAG,
};
use crate::ui::{Icon, Link, Svg};

const LICENSING: &str = include_str!("../../../../docs/legal/LICENSING.md");
const TERMS: &str = include_str!("../../../../docs/legal/TERMS.md");
const PRIVACY: &str = include_str!("../../../../docs/legal/PRIVACY.md");
/// The notices `scripts/legal-notices.py` writes: what the bundles ship,
/// this binary's crates among them.
const NOTICES: &str =
    include_str!("../../../desktop/src/views/settings/about/notices.generated.json");

const HOME: &str = "https://github.com/krazyjakee/caskade";
const ISSUES: &str = "https://github.com/krazyjakee/caskade/issues";
const RELEASES: &str = "https://github.com/krazyjakee/caskade/releases";

/// The Legal view's sections, each at `?tab=about&section=<id>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegalSection {
    Licenses,
    Packages,
    Terms,
    Privacy,
}

impl LegalSection {
    const ALL: [Self; 4] = [Self::Licenses, Self::Packages, Self::Terms, Self::Privacy];

    fn id(self) -> &'static str {
        match self {
            Self::Licenses => "licenses",
            Self::Packages => "packages",
            Self::Terms => "terms",
            Self::Privacy => "privacy",
        }
    }

    /// `value` when it names a section; About itself otherwise.
    pub fn of(value: Option<&str>) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|section| Some(section.id()) == value)
    }

    fn label(self) -> String {
        match self {
            Self::Licenses => t!("Licenses"),
            Self::Packages => t!("Open-source packages"),
            Self::Terms => t!("Terms and conditions"),
            Self::Privacy => t!("Privacy policy"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Licenses => SCALE,
            Self::Packages => PACKAGE_OPEN,
            Self::Terms => FILE_TEXT,
            Self::Privacy => SHIELD_CHECK,
        }
    }
}

/// A document's file name in `docs/legal/` → its section: a link between
/// them opens it.
fn document_section(href: &str) -> Option<LegalSection> {
    match href {
        "LICENSING.md" => Some(LegalSection::Licenses),
        "TERMS.md" => Some(LegalSection::Terms),
        "PRIVACY.md" => Some(LegalSection::Privacy),
        _ => None,
    }
}

fn open_page(href: &str) {
    if let Ok(url) = url::Url::parse(href) {
        caskade_app::navigation::open_in_browser(&url);
    }
}

/// Settings → About; `section` opens a Legal section in its place.
#[component]
pub fn AboutSettings(section: Option<String>) -> Element {
    match LegalSection::of(section.as_deref()) {
        Some(section) => rsx! {
            LegalView { section }
        },
        None => rsx! {
            AboutPage {}
        },
    }
}

fn href(section: Option<LegalSection>) -> String {
    match section {
        Some(section) => format!("/settings?tab=about&section={}", section.id()),
        None => "/settings?tab=about".to_owned(),
    }
}

/// The data services Caskade credits (#162): equal billing, side by side.
fn credits() -> [(&'static str, &'static str, &'static str, String, String); 4] {
    [
        (
            "IGDB",
            "igdb.com",
            "https://www.igdb.com/",
            tp!("credit", "Game metadata"),
            t!("Game details, artwork and matches come from the Internet Game Database."),
        ),
        (
            "Hasheous",
            "hasheous.org",
            "https://hasheous.org/",
            tp!("credit", "Game identification"),
            t!("Hashes and titles that identify games and ROMs come from Hasheous."),
        ),
        (
            "Ludusavi manifest",
            "github.com/mtkennerly/ludusavi-manifest",
            "https://github.com/mtkennerly/ludusavi-manifest",
            tp!("credit", "Save locations"),
            t!(
                "Save file and registry locations come from the Ludusavi manifest, an MIT-licensed project."
            ),
        ),
        (
            "PCGamingWiki",
            "pcgamingwiki.com",
            "https://www.pcgamingwiki.com/",
            tp!("credit", "Save location data"),
            t!(
                "The Ludusavi manifest includes data compiled from PCGamingWiki, licensed under CC BY-NC-SA 3.0."
            ),
        ),
    ]
}

/// This build: a release, or one for development.
fn build_label() -> String {
    if cfg!(debug_assertions) {
        t!("Debug build")
    } else {
        t!("Release build")
    }
}

const PANEL: &str = "rounded-panel border border-border bg-surface-raised p-5";
const LINK: &str = "inline-flex items-center gap-1.5 rounded-sm text-accent-alt underline-offset-2 data-hovered:underline data-focus-visible:focus-ring";

/// Settings → About: the app and its build, credits, and Legal.
#[component]
fn AboutPage() -> Element {
    let router = use_router();
    let projects = [
        (t!("Project page"), HOME, GIT_BRANCH),
        (t!("Report a problem"), ISSUES, BUG),
        (tp!("project link", "Releases"), RELEASES, TAG),
    ];
    rsx! {
        div { "data-cs": "about", class: "flex min-w-0 flex-col gap-6",
            h2 { "data-part": "title", class: "font-display text-2xl font-semibold tracking-wide",
                {tp!("settings tab", "About")}
            }
            section { "data-part": "details", "aria-labelledby": "about-caskade", class: PANEL,
                div { class: "flex items-start gap-4",
                    crate::frame::Brand { class: "size-12 shrink-0" }
                    div { class: "min-w-0",
                        h3 { id: "about-caskade", class: "font-display text-xl font-semibold tracking-wide", "Caskade" }
                        p { class: "mt-1 text-sm text-fg-muted",
                            {t!("A game library and launcher for Windows, Linux and macOS, with a desktop mode and a fullscreen console mode for gamepads.")}
                        }
                    }
                }
                dl { "data-part": "version", class: "mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm",
                    dt { class: "text-fg-subtle", {t!("Version")} }
                    dd { class: "font-mono text-fg", {env!("CARGO_PKG_VERSION")} }
                    dt { class: "text-fg-subtle", {tp!("app build", "Build")} }
                    dd { class: "text-fg", {build_label()} }
                }
                ul { "data-part": "links", class: "mt-4 flex flex-wrap gap-x-5 gap-y-2 text-sm",
                    li {
                        Link {
                            href: "/settings?tab=general&section=updates",
                            class: LINK,
                            on_press: move |_| router.replace("/settings?tab=general&section=updates"),
                            Svg { icon: CIRCLE_ARROW_UP, class: "size-4" }
                            {t!("Caskade updates")}
                        }
                    }
                    for (label, href, icon) in projects {
                        li { key: "{href}",
                            Link {
                                href,
                                class: LINK,
                                on_press: move |_| open_page(href),
                                Svg { icon, class: "size-4" }
                                "{label}"
                            }
                        }
                    }
                }
            }
            ul { "data-part": "credits", "aria-label": t!("Credits"), class: "grid gap-4 sm:grid-cols-2",
                for (name, host, page, role, description) in credits() {
                    li { key: "{name}", class: "flex",
                        Link {
                            href: page,
                            "data-part": "credit",
                            "aria-label": t!(
                                note = "A credit: 'Game metadata by IGDB (igdb.com)'",
                                "{role} by {name} ({host})",
                                role = role.clone(),
                                name = name,
                                host = host
                            ),
                            class: "flex flex-1 flex-col gap-1 rounded-panel border border-border bg-surface-raised p-5 transition-colors duration-(--cs-motion-duration-fast) data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover data-focus-visible:focus-ring",
                            on_press: move |_| open_page(page),
                            span { class: "font-mono text-xs tracking-wider text-fg-subtle uppercase", "{role}" }
                            span { class: "font-display text-2xl font-semibold tracking-wide text-fg", "{name}" }
                            span { class: "text-sm text-fg-muted", "{description}" }
                            span { class: "mt-2 inline-flex items-center gap-1.5 text-sm text-accent-alt",
                                "{host}"
                                Svg { icon: EXTERNAL_LINK, class: "size-3.5" }
                            }
                        }
                    }
                }
            }
            nav { "data-part": "legal-links", "aria-labelledby": "about-legal", class: PANEL,
                h3 { id: "about-legal", class: "font-display text-lg font-semibold tracking-wide", {t!("Legal")} }
                p { class: "text-sm text-fg-muted", {t!("Licenses, terms and privacy. They read offline.")} }
                ul { class: "mt-3 flex flex-col",
                    for section in LegalSection::ALL {
                        li { key: "{section.id()}", class: "border-t border-border first:border-t-0",
                            SectionLink { section: Some(section),
                                class: "flex items-center gap-3 rounded-control px-2 py-3 text-sm text-fg data-hovered:bg-surface-hover data-focus-visible:focus-ring",
                                Svg { icon: section.icon(), class: "size-4 shrink-0 text-fg-muted" }
                                span { class: "flex-1", {section.label()} }
                                Svg { icon: CHEVRON_RIGHT, class: "size-4 shrink-0 text-fg-subtle" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A link to a section of About, in place.
#[component]
fn SectionLink(
    section: Option<LegalSection>,
    #[props(default)] class: String,
    #[props(default)] current: bool,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let router = use_router();
    let to = href(section);
    rsx! {
        Link {
            href: to.clone(),
            class,
            current,
            on_press: move |_| router.replace(&to),
            attributes,
            {children}
        }
    }
}

/// Settings → About → Legal: its sections, the open one below.
#[component]
fn LegalView(section: LegalSection) -> Element {
    rsx! {
        div { "data-cs": "legal", class: "flex min-w-0 flex-col gap-4",
            SectionLink {
                section: None,
                "data-part": "back",
                class: "inline-flex w-fit items-center gap-1.5 rounded-control px-2 py-1 text-sm text-fg-muted data-hovered:bg-surface-hover data-hovered:text-fg data-focus-visible:focus-ring",
                Svg { icon: ARROW_LEFT, class: "size-4" }
                {t!("Back to About")}
            }
            h2 { "data-part": "title", class: "font-display text-2xl font-semibold tracking-wide", {t!("Legal")} }
            nav { "data-part": "nav", "aria-label": t!("Legal sections"),
                ul { class: "flex flex-wrap gap-2",
                    for each in LegalSection::ALL {
                        li { key: "{each.id()}",
                            SectionLink {
                                section: Some(each),
                                current: each == section,
                                "data-part": "section-link",
                                "data-focus-default": (each == section).then_some("true"),
                                class: "block rounded-control border border-border px-3 py-1.5 text-sm text-fg-muted data-hovered:bg-surface-hover data-hovered:text-fg data-current:border-accent-alt/50 data-current:bg-accent/20 data-current:text-fg data-focus-visible:focus-ring",
                                {each.label()}
                            }
                        }
                    }
                }
            }
            match section {
                LegalSection::Licenses => rsx! { Licenses {} },
                LegalSection::Packages => rsx! { Packages {} },
                LegalSection::Terms => rsx! {
                    Document { label: section.label(), Markdown { source: TERMS } }
                },
                LegalSection::Privacy => rsx! {
                    Document { label: section.label(), Markdown { source: PRIVACY } }
                },
            }
        }
    }
}

/// A long text: a stop of its own, so a pad and the keyboard scroll it.
#[component]
fn Document(label: String, children: Element) -> Element {
    rsx! {
        article {
            "data-part": "document",
            "aria-label": "{label}",
            tabindex: "0",
            "data-spatial": "focusable",
            class: "min-w-0 rounded-panel border border-border bg-surface-raised p-5 break-words sm:p-6",
            {children}
        }
    }
}

#[derive(Deserialize)]
struct Notices {
    #[serde(rename = "sevenZip")]
    seven_zip: SevenZip,
    groups: Vec<Group>,
    texts: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SevenZip {
    version: String,
    licenses: Vec<SevenZipLicense>,
}

#[derive(Deserialize)]
struct SevenZipLicense {
    platforms: Vec<String>,
    text: String,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Group {
    license: String,
    packages: Vec<Package>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Package {
    name: String,
    version: String,
    ecosystem: String,
    #[serde(default)]
    copyright: Vec<String>,
    #[serde(default)]
    repository: String,
    #[serde(default)]
    notice: Option<String>,
}

/// The notices, read when Legal first shows them.
fn notices() -> &'static Notices {
    static NOTICES_READ: std::sync::OnceLock<Notices> = std::sync::OnceLock::new();
    NOTICES_READ.get_or_init(|| serde_json::from_str(NOTICES).expect("the generated notices"))
}

fn license_text(title: String, text: String) -> Element {
    rsx! {
        h4 { class: "mt-6 text-base font-semibold text-fg", "{title}" }
        pre { class: "mt-2 rounded-control bg-surface-base/50 px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words text-fg-muted",
            "{text}"
        }
    }
}

/// Caskade's licensing and 7-Zip's licences, in English as published.
#[component]
fn Licenses() -> Element {
    let seven_zip = &notices().seven_zip;
    rsx! {
        Document { label: t!("Licenses"),
            Markdown { source: LICENSING }
            for licence in seven_zip.licenses.iter() {
                {license_text(
                    t!(
                        "7-Zip {version} licence ({platforms})",
                        version = seven_zip.version.clone(),
                        platforms = licence.platforms.join(", ")
                    ),
                    licence.text.clone(),
                )}
            }
        }
    }
}

/// The licences an expression names: `(MIT OR Apache-2.0) AND Unicode-3.0`
/// → three.
fn licence_terms(expression: &str) -> Vec<String> {
    let cleaned = expression.replace(['(', ')'], " ");
    cleaned
        .split(" OR ")
        .flat_map(|part| part.split(" AND "))
        .map(|term| term.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|term| !term.is_empty())
        .collect()
}

#[component]
fn PackageGroup(group: Group) -> Element {
    let mut open = use_signal(|| false);
    let count = group.packages.len();
    let texts = &notices().texts;
    rsx! {
        li { "data-part": "license-group", class: "rounded-control border border-border bg-surface-raised/60",
            button {
                r#type: "button",
                "aria-expanded": if open() { "true" } else { "false" },
                "data-spatial": "focusable",
                class: "flex w-full items-center gap-3 rounded-control px-4 py-3 text-left hover:bg-surface-hover focus-visible:focus-ring",
                onclick: move |_| open.toggle(),
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Named(NamedKey::Enter) || event.key() == Key::Character(" ".into()) {
                        event.prevent_default();
                        open.toggle();
                    }
                },
                span { class: "min-w-0 flex-1 font-mono text-sm break-words text-fg", "{group.license}" }
                span { class: "shrink-0 text-xs text-fg-muted",
                    {tn!("# package", "# packages", count as u64)}
                }
                Svg { icon: CHEVRON_DOWN, class: if open() { "size-4 shrink-0 rotate-180 text-fg-muted transition-transform" } else { "size-4 shrink-0 text-fg-muted transition-transform" } }
            }
            if open() {
                div { class: "border-t border-border p-2",
                    Document { label: t!("Packages under {license}", license = group.license.clone()),
                        ul { class: "flex flex-col gap-3 text-sm",
                            for item in group.packages.iter() {
                                li { key: "{item.ecosystem}:{item.name}@{item.version}", class: "min-w-0",
                                    span { class: "font-mono text-fg", "{item.name} {item.version}" }
                                    span { class: "text-xs text-fg-muted",
                                        " · "
                                        if item.ecosystem == "rust" { {t!("Rust crate")} } else { {t!("JavaScript package")} }
                                    }
                                    if !item.copyright.is_empty() {
                                        span { class: "block text-xs text-fg-muted", {item.copyright.join(" · ")} }
                                    }
                                    if !item.repository.is_empty() {
                                        span { class: "block font-mono text-xs break-all text-fg-muted", "{item.repository}" }
                                    }
                                    if let Some(notice) = item.notice.clone() {
                                        pre { class: "mt-1 font-mono text-xs whitespace-pre-wrap break-words text-fg-muted", "{notice}" }
                                    }
                                }
                            }
                        }
                        for term in licence_terms(&group.license) {
                            match texts.get(&term) {
                                Some(text) => license_text(term.clone(), text.clone()),
                                None => rsx! {
                                    p { class: "mt-6 text-sm text-fg-muted",
                                        if group.license.contains(" OR ") {
                                            {t!("{term} is one of the choices these packages offer; Caskade uses another, so its text isn't bundled.", term = term.clone())}
                                        } else {
                                            {t!("{term}: its text isn't bundled; see the packages' repositories.", term = term.clone())}
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Packages() -> Element {
    let notices = notices();
    let all: Vec<&Package> = notices
        .groups
        .iter()
        .flat_map(|group| &group.packages)
        .collect();
    let rust = all.iter().filter(|item| item.ecosystem == "rust").count();
    rsx! {
        div { class: "flex min-w-0 flex-col gap-4",
            div { class: "text-sm leading-relaxed text-fg-muted",
                h3 { class: "font-display text-2xl font-semibold tracking-wide text-fg", {t!("Open-source packages")} }
                p { class: "mt-3",
                    {t!("Caskade is built with {crates} Rust crates and {packages} JavaScript packages, grouped here by licence. Open a licence for its packages, their copyright holders and the licence's text. Where a licence reads \"A OR B\", the package may be used under either.", crates = rust, packages = all.len() - rust)}
                }
            }
            ul { "aria-label": t!("Licences"), class: "flex flex-col gap-2",
                for group in notices.groups.iter() {
                    PackageGroup { key: "{group.license}", group: group.clone() }
                }
            }
        }
    }
}

// ── The Markdown `docs/legal/` uses, and no more: headings, paragraphs,
// lists, quotes and fenced code; links, bold and inline code. ──

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Heading(usize, String),
    Paragraph(String),
    List(Vec<String>),
    Quote(String),
    Code(String),
}

fn is_list_item(line: &str) -> bool {
    (line.starts_with("- ") || line.starts_with("* ")) && line.len() > 2
}

fn starts_block(line: &str) -> bool {
    heading(line).is_some()
        || line.starts_with('>')
        || is_list_item(line)
        || line.starts_with("```")
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) && line[level..].starts_with(char::is_whitespace) {
        Some((level, line[level..].trim()))
    } else {
        None
    }
}

fn parse_markdown(source: &str) -> Vec<Block> {
    let source = source.replace("\r\n", "\n");
    let lines: Vec<&str> = source.split('\n').collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if let Some((level, text)) = heading(line) {
            blocks.push(Block::Heading(level, text.to_owned()));
            i += 1;
        } else if line.starts_with("```") {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].starts_with("```") {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            blocks.push(Block::Code(body.join("\n")));
        } else if line.starts_with('>') {
            let mut body = Vec::new();
            while i < lines.len() && lines[i].starts_with('>') {
                let text = lines[i].strip_prefix('>').unwrap_or_default();
                body.push(text.strip_prefix(' ').unwrap_or(text));
                i += 1;
            }
            blocks.push(Block::Quote(body.join(" ")));
        } else if is_list_item(line) {
            let mut items: Vec<String> = Vec::new();
            while i < lines.len() {
                let next = lines[i];
                if is_list_item(next) {
                    items.push(next[2..].trim_start().to_owned());
                } else if next.starts_with("  ") && !next.trim().is_empty() {
                    let last = items.pop().unwrap_or_default();
                    items.push(format!("{last} {}", next.trim()));
                } else {
                    break;
                }
                i += 1;
            }
            blocks.push(Block::List(items));
        } else {
            let mut body = Vec::new();
            while i < lines.len() && !lines[i].trim().is_empty() && !starts_block(lines[i]) {
                body.push(lines[i].trim());
                i += 1;
            }
            blocks.push(Block::Paragraph(body.join(" ")));
        }
    }
    blocks
}

#[derive(Clone, Debug, PartialEq)]
enum Inline {
    Text(String),
    Link(String, Vec<Inline>),
    Bold(Vec<Inline>),
    Code(String),
}

/// `[label](href)`, `**bold**` and `` `code` ``, the rest as text.
fn parse_inline(text: &str) -> Vec<Inline> {
    let mut parts = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close + 2..].find(')')
            && !rest[close + 2..close + 2 + end].contains(char::is_whitespace)
        {
            let label = &rest[1..close];
            let href = &rest[close + 2..close + 2 + end];
            if !label.contains(']') {
                if !plain.is_empty() {
                    parts.push(Inline::Text(std::mem::take(&mut plain)));
                }
                parts.push(Inline::Link(href.to_owned(), parse_inline(label)));
                rest = &rest[close + 3 + end..];
                continue;
            }
        }
        if rest.starts_with("**")
            && let Some(end) = rest[2..].find("**")
            && end > 0
        {
            if !plain.is_empty() {
                parts.push(Inline::Text(std::mem::take(&mut plain)));
            }
            parts.push(Inline::Bold(parse_inline(&rest[2..2 + end])));
            rest = &rest[4 + end..];
            continue;
        }
        if c == '`'
            && let Some(end) = rest[1..].find('`')
            && end > 0
        {
            if !plain.is_empty() {
                parts.push(Inline::Text(std::mem::take(&mut plain)));
            }
            parts.push(Inline::Code(rest[1..1 + end].to_owned()));
            rest = &rest[2 + end..];
            continue;
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if !plain.is_empty() {
        parts.push(Inline::Text(plain));
    }
    parts
}

fn render_inline(parts: Vec<Inline>) -> Element {
    rsx! {
        for part in parts {
            match part {
                Inline::Text(text) => rsx! { "{text}" },
                Inline::Bold(inner) => rsx! {
                    strong { class: "font-semibold text-fg", {render_inline(inner)} }
                },
                Inline::Code(code) => rsx! {
                    code { class: "rounded-sm bg-surface-overlay px-1 font-mono text-[0.9em] break-all", "{code}" }
                },
                Inline::Link(href, inner) => rsx! { DocLink { href, {render_inline(inner)} } },
            }
        }
    }
}

/// A link in a document: another document opens its section; a web page
/// opens in the browser; a path in the repository is just text.
#[component]
fn DocLink(href: String, children: Element) -> Element {
    if let Some(section) = document_section(&href) {
        return rsx! {
            SectionLink { section: Some(section), class: "rounded-sm text-accent-alt underline-offset-2 data-hovered:underline data-focus-visible:focus-ring", {children} }
        };
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        let page = href.clone();
        return rsx! {
            Link { href, class: "rounded-sm text-accent-alt underline-offset-2 data-hovered:underline data-focus-visible:focus-ring", on_press: move |_| open_page(&page), {children} }
        };
    }
    rsx! {
        {children}
    }
}

const HEADING_STYLES: [&str; 3] = [
    "font-display text-2xl font-semibold tracking-wide text-fg",
    "mt-8 font-display text-lg font-semibold tracking-wide text-fg",
    "mt-6 text-base font-semibold text-fg",
];

/// A legal document from `docs/legal/`: its `#` heading becomes an h3 and
/// the others follow below it.
#[component]
fn Markdown(source: &'static str) -> Element {
    rsx! {
        div { class: "flex flex-col gap-3 text-sm leading-relaxed text-fg-muted",
            for block in parse_markdown(source) {
                match block {
                    Block::Heading(level, text) => {
                        let style = HEADING_STYLES[(level - 1).min(HEADING_STYLES.len() - 1)];
                        let inner = render_inline(parse_inline(&text));
                        match level {
                            1 => rsx! { h3 { class: style, {inner} } },
                            2 => rsx! { h4 { class: style, {inner} } },
                            3 => rsx! { h5 { class: style, {inner} } },
                            _ => rsx! { h6 { class: style, {inner} } },
                        }
                    }
                    Block::Paragraph(text) => rsx! { p { {render_inline(parse_inline(&text))} } },
                    Block::List(items) => rsx! {
                        ul { class: "flex list-disc flex-col gap-2 pl-5 marker:text-accent-alt",
                            for item in items {
                                li { {render_inline(parse_inline(&item))} }
                            }
                        }
                    },
                    Block::Quote(text) => rsx! {
                        blockquote { class: "rounded-control border border-warning/40 bg-warning/10 px-4 py-3 text-fg",
                            {render_inline(parse_inline(&text))}
                        }
                    },
                    Block::Code(text) => rsx! {
                        pre { class: "rounded-control bg-surface-base/50 px-3 py-2 font-mono text-xs whitespace-pre-wrap break-words", "{text}" }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Inline, licence_terms, parse_inline, parse_markdown};

    #[test]
    fn markdown_as_the_legal_documents_use_it() {
        let blocks = parse_markdown(
            "# Terms\n\nSome **bold** text\nwrapped.\n\n- one\n  more\n- two\n\n> A note\n> here\n\n```\ncode\n```\n",
        );
        assert_eq!(
            blocks,
            [
                Block::Heading(1, "Terms".into()),
                Block::Paragraph("Some **bold** text wrapped.".into()),
                Block::List(vec!["one more".into(), "two".into()]),
                Block::Quote("A note here".into()),
                Block::Code("code".into()),
            ]
        );
        assert_eq!(
            parse_inline("See [the terms](TERMS.md), **this** and `that`."),
            [
                Inline::Text("See ".into()),
                Inline::Link("TERMS.md".into(), vec![Inline::Text("the terms".into())]),
                Inline::Text(", ".into()),
                Inline::Bold(vec![Inline::Text("this".into())]),
                Inline::Text(" and ".into()),
                Inline::Code("that".into()),
                Inline::Text(".".into()),
            ]
        );
    }

    #[test]
    fn a_licence_expression_names_its_terms() {
        assert_eq!(
            licence_terms("(MIT OR Apache-2.0) AND Unicode-3.0"),
            ["MIT", "Apache-2.0", "Unicode-3.0"]
        );
    }

    #[test]
    fn the_notices_read() {
        let notices = super::notices();
        assert!(!notices.groups.is_empty());
        assert!(!notices.seven_zip.licenses.is_empty());
    }
}
