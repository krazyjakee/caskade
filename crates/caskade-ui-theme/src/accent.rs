//! A user accent (Appearance → Accent, #141): its derived tones, kept
//! readable against the active theme's surfaces. The webview's
//! `appearance/accent.ts`, number for number. Colours are `#rrggbb`.

pub type Rgb = [f64; 3];

/// `#rrggbb` (the `#` optional).
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([
        f64::from((value >> 16) & 255),
        f64::from((value >> 8) & 255),
        f64::from(value & 255),
    ])
}

/// `#rrggbb` or `#rrggbbaa` composited over `under` (for translucent
/// surfaces).
pub fn parse_surface(text: &str, under: Rgb) -> Option<Rgb> {
    let hex = text.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let (colour, alpha) = match hex.len() {
        6 => (hex, 1.0),
        8 => {
            let alpha = u8::from_str_radix(hex.get(6..)?, 16).ok()?;
            (hex.get(..6)?, f64::from(alpha) / 255.0)
        }
        _ => return None,
    };
    let base = parse_hex(colour)?;
    Some(std::array::from_fn(|i| {
        (base[i] * alpha + under[i] * (1.0 - alpha)).round()
    }))
}

/// `#rrggbb`, each channel rounded and clamped.
pub fn hex(rgb: Rgb) -> String {
    let channel = |value: f64| value.clamp(0.0, 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgb[0]),
        channel(rgb[1]),
        channel(rgb[2])
    )
}

fn linear(channel: f64) -> f64 {
    let c = channel / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(c: f64) -> f64 {
    255.0
        * if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055
        }
}

/// WCAG 2 contrast ratio.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let luminance = |[r, g, b]: Rgb| 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    let (x, y) = (luminance(a), luminance(b));
    let (high, low) = if x >= y { (x, y) } else { (y, x) };
    (high + 0.05) / (low + 0.05)
}

#[derive(Debug, Clone, Copy)]
struct Oklch {
    l: f64,
    c: f64,
    h: f64,
}

fn to_oklch([r, g, b]: Rgb) -> Oklch {
    let (lr, lg, lb) = (linear(r), linear(g), linear(b));
    let l = (0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb).cbrt();
    let m = (0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb).cbrt();
    let s = (0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb).cbrt();
    let big_l = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    Oklch {
        l: big_l,
        c: a.hypot(b),
        h: b.atan2(a),
    }
}

fn from_oklch(Oklch { l: big_l, c, h }: Oklch) -> Rgb {
    let a = c * h.cos();
    let b = c * h.sin();
    let l = (big_l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (big_l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (big_l - 0.0894841775 * a - 1.291485548 * b).powi(3);
    [
        gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        gamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    ]
}

/// Non-text UI (selection, rails, primary buttons) against its surface.
pub const MIN_CONTRAST: f64 = 3.0;

/// The tones to apply: `--cs-color-accent`, `-strong`, `-fg` and
/// `--cs-color-glow`.
#[derive(Debug, Clone, PartialEq)]
pub struct Accent {
    pub accent: String,
    pub strong: String,
    pub fg: String,
    pub glow: String,
    /// Set when the colour had to change to stay readable.
    pub message: Option<String>,
}

impl Accent {
    /// The tokens it sets, in the order the webview sets them.
    pub fn tokens(&self) -> [(&'static str, &str); 4] {
        [
            ("--cs-color-accent", &self.accent),
            ("--cs-color-accent-strong", &self.strong),
            ("--cs-color-accent-fg", &self.fg),
            ("--cs-color-glow", &self.glow),
        ]
    }
}

/// The tones for `chosen` on `surfaces` (the theme's base and raised
/// colours, opaque). An accent below 3:1 against any surface is moved in
/// OKLCH lightness, away from the surfaces, until it passes; one that can't
/// pass is refused with the reason.
pub fn derive(chosen: &str, surfaces: &[Rgb]) -> Result<Accent, String> {
    let Some(rgb) = parse_hex(chosen) else {
        return Err(caskade_core::tr!(
            "{colour} is not a #rrggbb colour",
            colour = chosen
        ));
    };
    if surfaces.is_empty() {
        return Err(caskade_core::tr!("The theme's surfaces are unknown"));
    }
    let worst = |colour: Rgb| {
        surfaces
            .iter()
            .map(|surface| contrast(colour, *surface))
            .fold(f64::INFINITY, f64::min)
    };
    let lch = to_oklch(rgb);
    let dark_theme = surfaces.iter().all(|surface| to_oklch(*surface).l < 0.5);
    let mut accent = rgb;
    let mut adjusted = false;
    let mut step = 0.0;
    while worst(accent) < MIN_CONTRAST {
        let l = lch.l + if dark_theme { 0.02 } else { -0.02 } * (step + 1.0);
        if l <= 0.0 || l >= 1.0 {
            return Err(refuse(chosen));
        }
        accent = from_oklch(Oklch { l, ..lch });
        adjusted = true;
        step += 1.0;
    }
    let tone = to_oklch(accent);
    // Like the default accent: a lighter, slightly shifted "strong" tone and
    // a translucent glow.
    let strong = from_oklch(Oklch {
        l: (tone.l + if dark_theme { 0.06 } else { -0.06 }).min(0.95),
        c: tone.c,
        h: tone.h + 0.12,
    });
    // Button labels on the accent: the better of white and near-black.
    let fg = ["#ffffff", "#0b0e1a"]
        .into_iter()
        .map(|candidate| {
            let ratio = parse_hex(candidate).map_or(0.0, |colour| contrast(colour, accent));
            (candidate, ratio)
        })
        .fold(None::<(&str, f64)>, |best, next| match best {
            Some(best) if best.1 >= next.1 => Some(best),
            _ => Some(next),
        });
    let Some((fg, ratio)) = fg else {
        return Err(refuse(chosen));
    };
    if ratio < MIN_CONTRAST {
        return Err(refuse(chosen));
    }
    let accent_hex = hex(accent);
    let message = adjusted.then(|| {
        if dark_theme {
            caskade_core::tr!(
                "{colour} was lightened to {accent} to stay readable on this theme.",
                colour = chosen,
                accent = accent_hex
            )
        } else {
            caskade_core::tr!(
                "{colour} was darkened to {accent} to stay readable on this theme.",
                colour = chosen,
                accent = accent_hex
            )
        }
    });
    let glow = from_oklch(Oklch {
        l: (tone.l + 0.04).min(0.95),
        ..tone
    });
    Ok(Accent {
        accent: accent_hex,
        strong: hex(strong),
        fg: fg.to_owned(),
        glow: format!("{}80", hex(glow)),
        message,
    })
}

fn refuse(chosen: &str) -> String {
    caskade_core::tr!(
        "{colour} can't be made readable on this theme's surfaces; pick another colour.",
        colour = chosen
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default theme's surfaces: base, and raised composited over base.
    fn dark() -> Vec<Rgb> {
        let base = parse_surface("#060914", [0.0; 3]).unwrap();
        let raised = parse_surface("#0d1224d9", base).unwrap();
        vec![base, raised]
    }

    #[test]
    fn keeps_a_readable_accent_and_derives_its_tones() {
        let result = derive("#5b5ff7", &dark()).unwrap();
        assert_eq!(result.accent, "#5b5ff7");
        assert_eq!(result.fg, "#ffffff");
        assert_eq!(result.message, None);
        assert!(result.glow.len() == 9 && result.glow.ends_with("80"));
        assert_ne!(result.strong, result.accent);
    }

    #[test]
    fn lightens_an_accent_that_would_vanish_on_a_dark_theme_and_says_so() {
        let result = derive("#1a1f4a", &dark()).unwrap();
        assert_ne!(result.accent, "#1a1f4a");
        let message = result.message.unwrap();
        assert!(message.contains("lightened to #"), "{message}");
        let accent = parse_hex(&result.accent).unwrap();
        for surface in dark() {
            assert!(contrast(accent, surface) >= MIN_CONTRAST);
        }
    }

    #[test]
    fn darkens_on_a_light_theme_and_refuses_what_cant_be_made_readable() {
        let light = derive("#ffe14d", &[parse_hex("#ffffff").unwrap()]).unwrap();
        assert!(light.message.unwrap().contains("darkened"));
        // No colour reaches 3:1 against both of these mid surfaces.
        let mid = [parse_hex("#3a3a3a").unwrap(), parse_hex("#c8c8c8").unwrap()];
        assert!(derive("#5b5ff7", &mid).is_err());
        assert!(derive("orange", &dark()).is_err());
    }

    /// WCAG AA for text: body and secondary text.
    const AA_TEXT: f64 = 4.5;
    /// WCAG AA for large text and non-text: the floor a pair on
    /// [`BELOW_AA`] still has to keep.
    const AA_LARGE: f64 = 3.0;

    /// The text tokens, each over every surface it's drawn on.
    const TEXT: [&str; 3] = [
        "--cs-color-fg",
        "--cs-color-fg-muted",
        "--cs-color-fg-subtle",
    ];
    const SURFACES: [&str; 3] = [
        "--cs-color-surface-base",
        "--cs-color-surface-raised",
        "--cs-color-surface-overlay",
    ];

    /// A built-in theme's text token known to miss 4.5:1, with its ratios
    /// when it was found (#153). It still has to reach 3:1 on every
    /// surface; whether to change the theme's colour or keep the token for
    /// large text only is the maintainers' call, so the entry stays until
    /// then. An entry that no longer misses fails, so the list can't go
    /// stale.
    const BELOW_AA: &[(&str, &str, &str)] = &[(
        "caskade.default",
        "--cs-color-fg-subtle",
        "Nexus's hints and captions, #6a7194: 4.17:1 on surface-base, 3.93:1 on \
         surface-raised, 3.61:1 on surface-overlay (Ember, from the catalogue, \
         inherits them)",
    )];

    /// `css` without its comments.
    fn uncommented(mut css: &str) -> String {
        let mut out = String::with_capacity(css.len());
        while let Some(start) = css.find("/*") {
            out.push_str(&css[..start]);
            css = css[start..]
                .find("*/")
                .map_or("", |end| &css[start + end + 2..]);
        }
        out.push_str(css);
        out
    }

    /// The declarations of the first `:root { … }` block in `css`.
    fn root_tokens(css: &str) -> Vec<(String, String)> {
        let css = &uncommented(css);
        let Some(start) = css.find(":root {") else {
            return Vec::new();
        };
        let block = &css[start + ":root {".len()..];
        let block = &block[..block.find('}').unwrap_or(block.len())];
        block
            .split(';')
            .filter_map(|declaration| {
                let (name, value) = declaration.split_once(':')?;
                let name = name.trim();
                name.starts_with("--")
                    .then(|| (name.to_owned(), value.trim().to_owned()))
            })
            .collect()
    }

    /// `name`'s value in `tokens` (the last one wins), with `var(--x)`
    /// followed to what it names.
    fn token(tokens: &[(String, String)], name: &str) -> String {
        let mut value = tokens
            .iter()
            .rev()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {name}"));
        for _ in 0..8 {
            let Some(inner) = value
                .strip_prefix("var(")
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                break;
            };
            let named = inner
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
            value = token(tokens, &named);
        }
        value
    }

    /// Every built-in theme's own text over its own surfaces (#153): the
    /// contract's defaults (`packages/tokens/tokens.css`) under the
    /// theme's compiled tokens, as the window cascades them. A raised or
    /// translucent surface is composited over the base, as the accent check
    /// does; text over the surface it's on. The accent's label over the
    /// accent is text too.
    #[test]
    fn every_built_in_theme_keeps_its_text_readable_on_its_surfaces() {
        let defaults = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../packages/tokens/tokens.css"),
        )
        .unwrap();
        let defaults = root_tokens(&defaults);
        assert!(!defaults.is_empty(), "tokens.css has no :root block");
        // No folder of the user's: the built-in themes only.
        let themes = caskade_core::themes::ThemeService::new(
            std::env::temp_dir().join(format!("caskade-contrast-none-{}", std::process::id())),
        );
        let built_in: Vec<String> = themes
            .list()
            .into_iter()
            .filter(|info| info.source == caskade_core::themes::ThemeSource::Builtin)
            .map(|info| info.id)
            .collect();
        for id in [
            "caskade.default",
            "caskade.cobalt",
            "caskade.emerald",
            "caskade.engage",
        ] {
            assert!(built_in.iter().any(|known| known == id), "{built_in:?}");
        }
        let mut failures = Vec::new();
        let mut allowed_and_missed = Vec::new();
        for id in &built_in {
            let css = themes.stylesheet(id).unwrap();
            let mut tokens = defaults.clone();
            tokens.extend(root_tokens(&css));
            let colour = |name: &str, under: Rgb| {
                let value = token(&tokens, name);
                parse_surface(&value, under)
                    .unwrap_or_else(|| panic!("{id}'s {name} isn't a colour: {value}"))
            };
            let base = colour("--cs-color-surface-base", [0.0; 3]);
            let mut pairs = Vec::new();
            for surface in SURFACES {
                let under = colour(surface, base);
                for text in TEXT {
                    pairs.push((text, surface, contrast(colour(text, under), under)));
                }
            }
            let accent = colour("--cs-color-accent", base);
            pairs.push((
                "--cs-color-accent-fg",
                "--cs-color-accent",
                contrast(colour("--cs-color-accent-fg", accent), accent),
            ));
            for (text, surface, ratio) in pairs {
                let allowed = BELOW_AA
                    .iter()
                    .any(|(theme, token, _)| theme == id && *token == text);
                let floor = if allowed { AA_LARGE } else { AA_TEXT };
                if ratio < floor {
                    failures.push(format!(
                        "{id}: {text} on {surface} is {ratio:.2}:1, below {floor}:1"
                    ));
                }
                if allowed && ratio < AA_TEXT {
                    allowed_and_missed.push((id.clone(), text));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "text a theme draws too faintly:\n{}",
            failures.join("\n")
        );
        for (theme, token, why) in BELOW_AA {
            assert!(
                allowed_and_missed
                    .iter()
                    .any(|(id, text)| id == theme && text == token),
                "{theme}'s {token} reaches 4.5:1 everywhere now; drop it from BELOW_AA ({why})"
            );
        }
    }

    /// The webview's `deriveAccent` gives these for the same inputs (run in
    /// Node on 10 Nov 2026): both apps show the same accent.
    #[test]
    fn matches_the_webview_tone_for_tone() {
        let tones = |accent: Accent| (accent.accent, accent.strong, accent.fg, accent.glow);
        let owned = |[a, b, c, d]: [&str; 4]| (a.into(), b.into(), c.into(), d.into());
        for (chosen, surfaces, webview) in [
            (
                "#1a1f4a",
                dark(),
                ["#566091", "#6d6fa3", "#ffffff", "#616c9d80"],
            ),
            (
                "#5b5ff7",
                dark(),
                ["#5b5ff7", "#7d6dff", "#ffffff", "#656cff80"],
            ),
            (
                "#ffe14d",
                vec![[255.0; 3]],
                ["#ab8d00", "#8e8000", "#0b0e1a", "#b79a0080"],
            ),
        ] {
            assert_eq!(
                tones(derive(chosen, &surfaces).unwrap()),
                owned(webview),
                "{chosen}"
            );
        }
    }
}
