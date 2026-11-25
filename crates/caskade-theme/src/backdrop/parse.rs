//! Parsing and validating `backdrop.json` against the schema.

use super::{
    Blend, ColorValue, Composition, FILE, FRAME_RATES, IMAGE_TYPES, ImageSize, Kind, Layer, Length,
    LengthValue, MAX_IMAGES, MAX_LAYERS, MAX_LENGTH, MAX_SHADERS, Mask, Motion, NumberValue,
    PERIODS, Param, SHADER_INPUTS, STOPS, Stop, TimeValue, Unit, parse_color, parse_length,
    parse_time,
};
use crate::Problem;
use crate::contract::contract;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileDoc {
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    #[serde(rename = "$description", default)]
    _description: Option<String>,
    #[serde(default)]
    base: Option<Value>,
    #[serde(default)]
    frame_rate: Option<f64>,
    layers: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LayerDoc {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    opacity: Option<Value>,
    #[serde(default)]
    blend: Option<String>,
    #[serde(default)]
    motion: Option<MotionDoc>,
    #[serde(default)]
    mask: Option<MaskDoc>,
    // linear
    #[serde(default)]
    angle: Option<f64>,
    #[serde(default)]
    stops: Option<Vec<StopDoc>>,
    // radial, image
    #[serde(default)]
    at: Option<[Value; 2]>,
    #[serde(default)]
    size: Option<Value>,
    // grid
    #[serde(default)]
    color: Option<Value>,
    #[serde(default)]
    cell: Option<Value>,
    #[serde(default)]
    line: Option<Value>,
    // image, shader
    #[serde(default)]
    src: Option<String>,
    #[serde(default)]
    repeat: Option<bool>,
    // shader
    #[serde(default)]
    colors: Option<Vec<Value>>,
    #[serde(default)]
    numbers: Option<Vec<Value>>,
    #[serde(default)]
    animate: Option<bool>,
    #[serde(rename = "loop", default)]
    looping: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopDoc {
    color: Value,
    #[serde(default)]
    alpha: Option<Value>,
    at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionDoc {
    #[serde(rename = "type")]
    kind: String,
    by: [Value; 2],
    period: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaskDoc {
    at: [Value; 2],
    size: [Value; 2],
    from: String,
    to: String,
}

struct Check<'a> {
    files: &'a [String],
    read: &'a dyn crate::Files,
    problems: Vec<Problem>,
}

impl Check<'_> {
    fn fail(&mut self, at: &str, message: impl std::fmt::Display) {
        self.problems
            .push(Problem::new(FILE, format!("{at}: {message}")));
    }

    fn only(&mut self, at: &str, name: &str, present: bool, kinds: &str) {
        if present {
            self.fail(at, format!("{name} is for {kinds} layers"));
        }
    }

    /// A `{token}` reference of `kind`, as its custom property.
    fn token(&mut self, at: &str, text: &str, kind: &str) -> Option<Option<String>> {
        let Some(path) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) else {
            return Some(None);
        };
        let path = path.strip_suffix(".$root").unwrap_or(path);
        match contract().tokens.get(path) {
            None => {
                self.fail(
                    at,
                    format!("{{{path}}} is not a token of the theming contract"),
                );
                None
            }
            Some(token) if token.kind != kind => {
                self.fail(at, format!("{{{path}}} is a {}, not a {kind}", token.kind));
                None
            }
            Some(token) => Some(Some(token.name.clone())),
        }
    }

    fn color(&mut self, at: &str, value: &Value) -> Option<ColorValue> {
        let Some(text) = value.as_str() else {
            self.fail(at, "expected a colour");
            return None;
        };
        if let Some(token) = self.token(at, text, "color")? {
            return Some(Param::Token(token));
        }
        let literal = parse_color(text).filter(|_| text.starts_with('#') || text == "transparent");
        match literal {
            Some(rgba) => Some(Param::Literal(rgba)),
            None => {
                self.fail(
                    at,
                    format!(
                        "expected a hex colour, transparent or a {{color.*}} token, got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn number(
        &mut self,
        at: &str,
        value: Option<&Value>,
        default: f32,
        range: std::ops::RangeInclusive<f32>,
    ) -> Option<NumberValue> {
        let Some(value) = value else {
            return Some(Param::Literal(default));
        };
        if let Some(text) = value.as_str()
            && let Some(token) = self.token(at, text, "number")?
        {
            return Some(Param::Token(token));
        }
        match value.as_f64().map(|n| n as f32) {
            Some(n) if n.is_finite() && range.contains(&n) => Some(Param::Literal(n)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a number from {} to {} or a {{number}} token, got {value}",
                        range.start(),
                        range.end()
                    ),
                );
                None
            }
        }
    }

    fn length(&mut self, at: &str, value: &Value) -> Option<LengthValue> {
        let text = match value {
            Value::String(text) => text.as_str(),
            Value::Number(n) if n.as_f64() == Some(0.0) => "0",
            _ => {
                self.fail(at, format!("expected a length, got {value}"));
                return None;
            }
        };
        if let Some(token) = self.token(at, text, "dimension")? {
            return Some(Param::Token(token));
        }
        match parse_length(text) {
            Some(length) if length.value.abs() <= MAX_LENGTH => Some(Param::Literal(length)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a length in px, rem, em, %, vw or vh (or a {{dimension}} token), got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn pair(&mut self, at: &str, values: &[Value; 2]) -> Option<[LengthValue; 2]> {
        let x = self.length(&format!("{at}[0]"), &values[0]);
        let y = self.length(&format!("{at}[1]"), &values[1]);
        Some([x?, y?])
    }

    /// One length for both axes, or a pair.
    fn lengths(&mut self, at: &str, value: &Value) -> Option<[LengthValue; 2]> {
        match value {
            Value::Array(items) if items.len() == 2 => {
                self.pair(at, &[items[0].clone(), items[1].clone()])
            }
            Value::Array(_) => {
                self.fail(at, "expected two lengths");
                None
            }
            single => {
                let length = self.length(at, single)?;
                Some([length.clone(), length])
            }
        }
    }

    fn percent(&mut self, at: &str, text: &str) -> Option<f32> {
        match parse_length(text) {
            Some(Length {
                value,
                unit: Unit::Percent,
            }) if (0.0..=1000.0).contains(&value) => Some(value / 100.0),
            _ => {
                self.fail(at, format!("expected a percentage, got \"{text}\""));
                None
            }
        }
    }

    fn time(&mut self, at: &str, value: &Value) -> Option<TimeValue> {
        let Some(text) = value.as_str() else {
            self.fail(at, format!("expected a duration, got {value}"));
            return None;
        };
        if let Some(token) = self.token(at, text, "duration")? {
            return Some(Param::Token(token));
        }
        match parse_time(text) {
            Some(seconds) if PERIODS.contains(&seconds) => Some(Param::Literal(seconds)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a duration from 1s to 3600s (or a {{duration}} token), got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn stops(&mut self, at: &str, stops: Option<Vec<StopDoc>>) -> Option<Vec<Stop>> {
        let Some(stops) = stops else {
            self.fail(at, "a gradient needs stops");
            return None;
        };
        if !STOPS.contains(&stops.len()) {
            self.fail(at, "a gradient has 2 to 4 stops");
            return None;
        }
        let mut out = Vec::new();
        let mut last = f32::NEG_INFINITY;
        let mut ok = true;
        for (index, stop) in stops.into_iter().enumerate() {
            let at = format!("{at}[{index}]");
            let color = self.color(&format!("{at}.color"), &stop.color);
            let alpha = self.number(&format!("{at}.alpha"), stop.alpha.as_ref(), 1.0, 0.0..=1.0);
            let offset = self.percent(&format!("{at}.at"), &stop.at);
            if let Some(offset) = offset
                && offset < last
            {
                self.fail(&at, "stops go in order along the gradient");
                ok = false;
            }
            match (color, alpha, offset) {
                (Some(color), Some(alpha), Some(offset)) => {
                    last = offset;
                    out.push(Stop {
                        color,
                        alpha,
                        at: offset,
                    });
                }
                _ => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn image(&mut self, at: &str, src: Option<String>) -> Option<String> {
        let Some(src) = src else {
            self.fail(at, "an image layer needs a src");
            return None;
        };
        let extension = src
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        if !src.starts_with("assets/") || src.contains("..") || src.contains('\\') {
            self.fail(at, format!("\"{src}\" must be a file under assets/"));
            None
        } else if !IMAGE_TYPES.contains(&extension.as_str()) {
            self.fail(at, format!("\"{src}\" must be a PNG, JPEG or WebP"));
            None
        } else if !self.files.contains(&src) {
            self.fail(at, format!("\"{src}\" doesn't exist in the theme"));
            None
        } else {
            Some(src)
        }
    }

    fn shader(
        &mut self,
        at: &str,
        src: Option<String>,
    ) -> Option<(String, crate::shader::Compiled)> {
        let Some(src) = src else {
            self.fail(at, "a shader layer needs a src");
            return None;
        };
        if !src.starts_with("assets/") || src.contains("..") || src.contains('\\') {
            self.fail(at, format!("\"{src}\" must be a file under assets/"));
            return None;
        }
        if !src.to_ascii_lowercase().ends_with(".wgsl") {
            self.fail(at, format!("\"{src}\" must be a .wgsl file"));
            return None;
        }
        let Some(bytes) = self
            .files
            .contains(&src)
            .then(|| self.read.read(&src))
            .flatten()
        else {
            self.fail(at, format!("\"{src}\" doesn't exist in the theme"));
            return None;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            self.fail(at, format!("\"{src}\" is not UTF-8"));
            return None;
        };
        match crate::shader::compile(&src, &text) {
            Ok(compiled) => Some((src, compiled)),
            Err(mut problems) => {
                self.problems.append(&mut problems);
                None
            }
        }
    }

    fn shader_layer(&mut self, at: &str, doc: &mut LayerDoc) -> Option<Kind> {
        self.only(at, "angle", doc.angle.is_some(), "linear");
        self.only(at, "stops", doc.stops.is_some(), "linear and radial");
        self.only(at, "at", doc.at.is_some(), "radial and image");
        self.only(at, "size", doc.size.is_some(), "radial and image");
        self.only(at, "color", doc.color.is_some(), "grid");
        self.only(at, "cell", doc.cell.is_some(), "grid");
        self.only(at, "line", doc.line.is_some(), "grid");
        self.only(at, "repeat", doc.repeat.is_some(), "image");
        if doc.motion.is_some() {
            self.fail(
                &format!("{at}.motion"),
                "a shader moves itself: use animate and loop",
            );
        }
        for (name, list) in [("colors", &doc.colors), ("numbers", &doc.numbers)] {
            if list.as_ref().map_or(0, Vec::len) > SHADER_INPUTS {
                self.fail(&format!("{at}.{name}"), format!("at most {SHADER_INPUTS}"));
            }
        }
        let colors: Vec<Option<ColorValue>> = doc
            .colors
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, value)| self.color(&format!("{at}.colors[{index}]"), value))
            .collect();
        let numbers: Vec<Option<NumberValue>> = doc
            .numbers
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, value)| {
                self.number(
                    &format!("{at}.numbers[{index}]"),
                    Some(value),
                    0.0,
                    -MAX_LENGTH..=MAX_LENGTH,
                )
            })
            .collect();
        let shader = self.shader(&format!("{at}.src"), doc.src.take());
        match (
            shader,
            colors.into_iter().collect::<Option<Vec<_>>>(),
            numbers.into_iter().collect::<Option<Vec<_>>>(),
        ) {
            (Some((src, shader)), Some(colors), Some(numbers)) => Some(Kind::Shader {
                src,
                shader,
                colors,
                numbers,
            }),
            _ => None,
        }
    }

    fn linear_layer(&mut self, at: &str, doc: &mut LayerDoc) -> Option<Kind> {
        self.only(at, "at", doc.at.is_some(), "radial and image");
        self.only(at, "size", doc.size.is_some(), "radial and image");
        self.only(at, "color", doc.color.is_some(), "grid");
        self.only(at, "cell", doc.cell.is_some(), "grid");
        self.only(at, "line", doc.line.is_some(), "grid");
        self.only(at, "src", doc.src.is_some(), "image");
        self.only(at, "repeat", doc.repeat.is_some(), "image");
        let angle = doc.angle.unwrap_or(180.0) as f32;
        if !angle.is_finite() || angle.abs() > 3600.0 {
            self.fail(&format!("{at}.angle"), "expected degrees");
        }
        let stops = self.stops(&format!("{at}.stops"), doc.stops.take());
        Some(Kind::Linear {
            angle: angle.rem_euclid(360.0),
            stops: stops?,
        })
    }

    fn radial_layer(&mut self, at: &str, doc: &mut LayerDoc) -> Option<Kind> {
        self.only(at, "angle", doc.angle.is_some(), "linear");
        self.only(at, "color", doc.color.is_some(), "grid");
        self.only(at, "cell", doc.cell.is_some(), "grid");
        self.only(at, "line", doc.line.is_some(), "grid");
        self.only(at, "src", doc.src.is_some(), "image");
        self.only(at, "repeat", doc.repeat.is_some(), "image");
        let center = match &doc.at {
            Some(at_pair) => self.pair(&format!("{at}.at"), at_pair),
            None => Some([
                Param::Literal(Length {
                    value: 50.0,
                    unit: Unit::Percent,
                }),
                Param::Literal(Length {
                    value: 50.0,
                    unit: Unit::Percent,
                }),
            ]),
        };
        let size = match &doc.size {
            Some(size) => self.lengths(&format!("{at}.size"), size),
            None => {
                self.fail(at, "a radial layer needs a size");
                None
            }
        };
        let stops = self.stops(&format!("{at}.stops"), doc.stops.take());
        Some(Kind::Radial {
            at: center?,
            size: size?,
            stops: stops?,
        })
    }

    fn grid_layer(&mut self, at: &str, doc: &LayerDoc) -> Option<Kind> {
        self.only(at, "angle", doc.angle.is_some(), "linear");
        self.only(at, "stops", doc.stops.is_some(), "linear and radial");
        self.only(at, "at", doc.at.is_some(), "radial and image");
        self.only(at, "size", doc.size.is_some(), "radial and image");
        self.only(at, "src", doc.src.is_some(), "image");
        self.only(at, "repeat", doc.repeat.is_some(), "image");
        let color = match &doc.color {
            Some(color) => self.color(&format!("{at}.color"), color),
            None => {
                self.fail(at, "a grid needs a color");
                None
            }
        };
        let cell = match &doc.cell {
            Some(cell) => self.lengths(&format!("{at}.cell"), cell),
            None => {
                self.fail(at, "a grid needs a cell");
                None
            }
        };
        let line = match &doc.line {
            Some(line) => self.length(&format!("{at}.line"), line),
            None => Some(Param::Literal(Length {
                value: 1.0,
                unit: Unit::Px,
            })),
        };
        Some(Kind::Grid {
            color: color?,
            cell: cell?,
            line: line?,
        })
    }

    fn image_layer(&mut self, at: &str, doc: &mut LayerDoc) -> Option<Kind> {
        self.only(at, "angle", doc.angle.is_some(), "linear");
        self.only(at, "stops", doc.stops.is_some(), "linear and radial");
        self.only(at, "color", doc.color.is_some(), "grid");
        self.only(at, "cell", doc.cell.is_some(), "grid");
        self.only(at, "line", doc.line.is_some(), "grid");
        let src = self.image(&format!("{at}.src"), doc.src.take());
        let size = match &doc.size {
            None => Some(ImageSize::Auto),
            Some(Value::String(word)) if word == "cover" => Some(ImageSize::Cover),
            Some(Value::String(word)) if word == "contain" => Some(ImageSize::Contain),
            Some(Value::String(word)) if word == "auto" => Some(ImageSize::Auto),
            Some(size) => self
                .lengths(&format!("{at}.size"), size)
                .map(ImageSize::Explicit),
        };
        let position = match &doc.at {
            Some(at_pair) => self.pair(&format!("{at}.at"), at_pair),
            None => Some([
                Param::Literal(Length {
                    value: 0.0,
                    unit: Unit::Percent,
                }),
                Param::Literal(Length {
                    value: 0.0,
                    unit: Unit::Percent,
                }),
            ]),
        };
        Some(Kind::Image {
            src: src?,
            size: size?,
            at: position?,
            repeat: doc.repeat.unwrap_or(true),
        })
    }

    fn motion(&mut self, at: &str, doc: &LayerDoc, repeats: bool) -> Option<Option<Motion>> {
        match &doc.motion {
            None if doc.kind == "shader" && doc.animate != Some(false) => {
                let period = match &doc.looping {
                    Some(value) => self.time(&format!("{at}.loop"), value),
                    None => Some(Param::Literal(3600.0)),
                };
                period.map(|period| Some(Motion::Clock { period }))
            }
            None => Some(None),
            Some(_) if doc.kind == "shader" => Some(None),
            Some(motion) => {
                let at = format!("{at}.motion");
                let by = self.pair(&format!("{at}.by"), &motion.by);
                let period = self.time(&format!("{at}.period"), &motion.period);
                match motion.kind.as_str() {
                    "drift" => by
                        .zip(period)
                        .map(|(by, period)| Some(Motion::Drift { by, period })),
                    "pan" if repeats => by
                        .zip(period)
                        .map(|(by, period)| Some(Motion::Pan { by, period })),
                    "pan" => {
                        self.fail(
                            &at,
                            "pan moves repeating content only (a grid or a repeating image); use drift",
                        );
                        None
                    }
                    other => {
                        self.fail(
                            &format!("{at}.type"),
                            format!("\"{other}\" is not drift or pan"),
                        );
                        None
                    }
                }
            }
        }
    }

    fn mask(&mut self, at: &str, mask: Option<&MaskDoc>) -> Option<Option<Mask>> {
        let Some(mask) = mask else {
            return Some(None);
        };
        let at = format!("{at}.mask");
        let center = self.pair(&format!("{at}.at"), &mask.at);
        let size = self.pair(&format!("{at}.size"), &mask.size);
        let from = self.percent(&format!("{at}.from"), &mask.from);
        let to = self.percent(&format!("{at}.to"), &mask.to);
        if let (Some(from), Some(to)) = (from, to)
            && to <= from
        {
            self.fail(&at, "to must be beyond from");
        }
        match (center, size, from, to) {
            (Some(at), Some(size), Some(from), Some(to)) if to > from => {
                Some(Some(Mask { at, size, from, to }))
            }
            _ => None,
        }
    }

    fn layer(&mut self, at: &str, value: &Value) -> Option<Layer> {
        let mut doc: LayerDoc = match serde_json::from_value(value.clone()) {
            Ok(doc) => doc,
            Err(error) => {
                self.fail(at, error);
                return None;
            }
        };
        let opacity = self.number(
            &format!("{at}.opacity"),
            doc.opacity.as_ref(),
            1.0,
            0.0..=1.0,
        );
        let blend = match doc.blend.as_deref() {
            None | Some("normal") => Some(Blend::Normal),
            Some("multiply") => Some(Blend::Multiply),
            Some("screen") => Some(Blend::Screen),
            Some(other) => {
                self.fail(
                    &format!("{at}.blend"),
                    format!("\"{other}\" is not normal, multiply or screen"),
                );
                None
            }
        };
        if doc.kind != "shader" {
            self.only(at, "colors", doc.colors.is_some(), "shader");
            self.only(at, "numbers", doc.numbers.is_some(), "shader");
            self.only(at, "animate", doc.animate.is_some(), "shader");
            self.only(at, "loop", doc.looping.is_some(), "shader");
        }
        let kind = match doc.kind.as_str() {
            "shader" => self.shader_layer(at, &mut doc),
            "linear" => self.linear_layer(at, &mut doc),
            "radial" => self.radial_layer(at, &mut doc),
            "grid" => self.grid_layer(at, &doc),
            "image" => self.image_layer(at, &mut doc),
            other => {
                self.fail(
                    &format!("{at}.type"),
                    format!("\"{other}\" is not linear, radial, grid, image or shader"),
                );
                None
            }
        };
        let repeats = match &kind {
            Some(Kind::Grid { .. }) => true,
            Some(Kind::Image { repeat, .. }) => *repeat,
            None if doc.kind == "grid" => true,
            None if doc.kind == "image" => doc.repeat.unwrap_or(true),
            _ => false,
        };
        let motion = self.motion(at, &doc, repeats);
        let mask = self.mask(at, doc.mask.as_ref());
        Some(Layer {
            kind: kind?,
            opacity: opacity?,
            blend: blend?,
            motion: motion?,
            mask: mask?,
        })
    }
}

/// Checks `backdrop.json` (its text) against the schema. `files` are the
/// theme's, for the images it names.
pub(crate) fn parse(
    text: &str,
    files: &[String],
    read: &dyn crate::Files,
) -> Result<Composition, Vec<Problem>> {
    let doc: FileDoc =
        serde_json::from_str(text).map_err(|error| vec![Problem::json(FILE, &error)])?;
    let mut check = Check {
        files,
        read,
        problems: Vec::new(),
    };
    let base = match &doc.base {
        Some(base) => check.color("base", base),
        None => Some(Param::Token("--cs-color-surface-base".to_owned())),
    };
    let frame_rate = doc.frame_rate.map_or(30.0, |rate| rate as f32);
    if !FRAME_RATES.contains(&frame_rate) {
        check.fail("frameRate", "expected 1 to 60 frames a second");
    }
    if doc.layers.len() > MAX_LAYERS {
        check.fail(
            "layers",
            format!("a backdrop has at most {MAX_LAYERS} layers"),
        );
    }
    let layers: Vec<Option<Layer>> = doc
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| check.layer(&format!("layers[{index}]"), layer))
        .collect();
    let composition = Composition {
        base: base.unwrap_or(Param::Literal([0.0; 4])),
        frame_rate,
        layers: layers.into_iter().flatten().collect(),
    };
    let shader_layers = composition
        .layers
        .iter()
        .filter(|layer| matches!(layer.kind, Kind::Shader { .. }))
        .count();
    if shader_layers > MAX_SHADERS {
        check.fail(
            "layers",
            format!("a backdrop has at most {MAX_SHADERS} shader layers"),
        );
    }
    if composition.images().len() > MAX_IMAGES {
        check.fail(
            "layers",
            format!("a backdrop uses at most {MAX_IMAGES} images"),
        );
    }
    if check.problems.is_empty() {
        Ok(composition)
    } else {
        Err(check.problems)
    }
}
