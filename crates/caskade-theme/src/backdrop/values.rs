//! Parsers for computed CSS values and token defaults read back from the
//! backdrop's computed style.

use super::{Length, Rgba, Unit};
use crate::contract::contract;

/// A token's default value (its custom property, `--cs-grid-size`), aliases
/// resolved: what to use when a computed value can't be read.
pub fn token_default(name: &str) -> Option<&'static str> {
    contract()
        .tokens
        .values()
        .find(|token| token.name == name)
        .map(|token| token.resolved.as_str())
}

/// A CSS length: a number and one of px, rem, em, %, vw, vh (or `0`).
pub fn parse_length(text: &str) -> Option<Length> {
    let text = text.trim();
    if text == "0" {
        return Some(Length {
            value: 0.0,
            unit: Unit::Px,
        });
    }
    let split = text
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(text.len());
    let (amount, unit) = text.split_at(split);
    let value: f32 = amount.trim().parse().ok().filter(|n: &f32| n.is_finite())?;
    let unit = match unit.to_ascii_lowercase().as_str() {
        "px" => Unit::Px,
        "rem" => Unit::Rem,
        "em" => Unit::Em,
        "%" => Unit::Percent,
        "vw" => Unit::Vw,
        "vh" => Unit::Vh,
        _ => return None,
    };
    Some(Length { value, unit })
}

/// A CSS duration in seconds: `40s`, `500ms`.
pub fn parse_time(text: &str) -> Option<f32> {
    let text = text.trim();
    let (amount, scale) = if let Some(ms) = text.strip_suffix("ms") {
        (ms, 0.001)
    } else {
        (text.strip_suffix('s')?, 1.0)
    };
    let value: f32 = amount.trim().parse().ok()?;
    value.is_finite().then_some(value * scale)
}

/// A CSS number.
pub fn parse_number(text: &str) -> Option<f32> {
    text.trim().parse().ok().filter(|n: &f32| n.is_finite())
}

/// A CSS colour as a computed custom property holds it: `#rgb[a]`,
/// `#rrggbb[aa]`, `rgb()`/`rgba()`, `hsl()`/`hsla()` (comma or space
/// separated, alpha as a number or percentage), `transparent`, `black`
/// and `white`. Other syntaxes give `None`.
pub fn parse_color(text: &str) -> Option<Rgba> {
    let text = text.trim().to_ascii_lowercase();
    match text.as_str() {
        "transparent" => return Some([0.0; 4]),
        "black" => return Some([0.0, 0.0, 0.0, 1.0]),
        "white" => return Some([1.0; 4]),
        _ => {}
    }
    if let Some(digits) = text.strip_prefix('#') {
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |at: usize| u8::from_str_radix(&digits[at..=at], 16).ok();
        let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        let channels: Vec<u8> = match digits.len() {
            3 | 4 => (0..digits.len())
                .map(|at| nibble(at).map(|n| n * 17))
                .collect::<Option<_>>()?,
            6 | 8 => (0..digits.len() / 2)
                .map(|at| byte(at * 2))
                .collect::<Option<_>>()?,
            _ => return None,
        };
        let alpha = channels.get(3).map_or(1.0, |a| f32::from(*a) / 255.0);
        return Some([
            f32::from(channels[0]) / 255.0,
            f32::from(channels[1]) / 255.0,
            f32::from(channels[2]) / 255.0,
            alpha,
        ]);
    }
    let (name, rest) = text.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let (channels, alpha) = match inner.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha)),
        None => (inner, None),
    };
    let mut parts: Vec<&str> = channels
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    let alpha = match (alpha, parts.len()) {
        (Some(alpha), 3) => Some(alpha.trim()),
        (None, 4) => parts.pop(),
        (None, 3) => None,
        _ => return None,
    };
    let alpha = match alpha {
        None => 1.0,
        Some(alpha) => match alpha.strip_suffix('%') {
            Some(percent) => percent.trim().parse::<f32>().ok()? / 100.0,
            None => alpha.parse::<f32>().ok()?,
        },
    }
    .clamp(0.0, 1.0);
    let channel = |part: &str| -> Option<f32> {
        match part.strip_suffix('%') {
            Some(percent) => Some(percent.parse::<f32>().ok()? / 100.0),
            None => Some(part.parse::<f32>().ok()? / 255.0),
        }
    };
    let [r, g, b] = match name.trim() {
        "rgb" | "rgba" => [channel(parts[0])?, channel(parts[1])?, channel(parts[2])?],
        "hsl" | "hsla" => {
            let hue: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            let percent = |part: &str| -> Option<f32> {
                Some(part.strip_suffix('%').unwrap_or(part).parse::<f32>().ok()? / 100.0)
            };
            hsl(hue, percent(parts[1])?, percent(parts[2])?)
        }
        _ => return None,
    };
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    Some([clamp(r), clamp(g), clamp(b), alpha])
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let hue = hue.rem_euclid(360.0) / 30.0;
    let a = saturation.clamp(0.0, 1.0) * lightness.min(1.0 - lightness);
    let f = |n: f32| {
        let k = (n + hue) % 12.0;
        lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}
