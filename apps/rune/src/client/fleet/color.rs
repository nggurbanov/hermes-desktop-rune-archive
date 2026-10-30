//! Preserve authored CSS separately from its native paint conversion.
use rune::Color;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(super) enum Authored {
    Hue(u16),
    Css(String),
}
impl Authored {
    pub fn css(&self) -> String {
        match self {
            Self::Hue(hue) => hue_css(*hue),
            Self::Css(value) => value.clone(),
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Hue(hue) => *hue < 360,
            Self::Css(value) => !value.is_empty(),
        }
    }
    pub fn hue(&self) -> Option<u16> {
        match self {
            Self::Hue(hue) => Some(*hue),
            Self::Css(value) => (0..360).find(|hue| value == &hue_css(*hue)),
        }
    }
}
pub fn hue_css(hue: u16) -> String {
    format!("hsl({hue} 68% 58%)")
}
pub fn hue_color(hue: u16) -> Color {
    hsl(f32::from(hue), 0.68, 0.58, 1.)
}
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn rgb(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color {
        r: linear(r.clamp(0., 1.)),
        g: linear(g.clamp(0., 1.)),
        b: linear(b.clamp(0., 1.)),
        a: a.clamp(0., 1.),
    }
}
fn hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    let h = h.rem_euclid(360.) / 60.;
    let s = s.clamp(0., 1.);
    let l = l.clamp(0., 1.);
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    rgb(r + m, g + m, b + m, a)
}
fn number(value: &str) -> Option<f32> {
    value.parse::<f32>().ok().filter(|v| v.is_finite())
}
fn unit(value: &str, scale: f32) -> Option<f32> {
    if let Some(value) = value.strip_suffix('%') {
        Some(number(value)? / 100.)
    } else {
        Some(number(value)? / scale)
    }
}
fn hue(value: &str) -> Option<f32> {
    converted_hue(value).filter(|value| value.is_finite())
}
fn converted_hue(value: &str) -> Option<f32> {
    if let Some(value) = value.strip_suffix("turn") {
        Some(number(value)? * 360.)
    } else if let Some(value) = value.strip_suffix("grad") {
        Some(number(value)? * 0.9)
    } else if let Some(value) = value.strip_suffix("rad") {
        Some(number(value)?.to_degrees())
    } else {
        number(value.strip_suffix("deg").unwrap_or(value))
    }
}
pub(super) fn parse(value: &str) -> Option<Color> {
    parse_inner(value).filter(|c| [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite()))
}
fn parse_inner(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let component = |raw: &str| {
            u8::from_str_radix(raw, 16)
                .ok()
                .map(|v| f32::from(v) / 255.)
        };
        let expanded;
        let hex = if matches!(hex.len(), 3 | 4) {
            expanded = hex.chars().flat_map(|c| [c, c]).collect::<String>();
            expanded.as_str()
        } else {
            hex
        };
        if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
            return None;
        }
        return Some(rgb(
            component(&hex[..2])?,
            component(&hex[2..4])?,
            component(&hex[4..6])?,
            if hex.len() == 8 {
                component(&hex[6..8])?
            } else {
                1.
            },
        ));
    }
    let named: Option<(u8, u8, u8, f32)> = match value.as_str() {
        "transparent" => Some((0, 0, 0, 0.)),
        "black" => Some((0, 0, 0, 1.)),
        "white" => Some((255, 255, 255, 1.)),
        "red" => Some((255, 0, 0, 1.)),
        "lime" => Some((0, 255, 0, 1.)),
        "blue" => Some((0, 0, 255, 1.)),
        "green" => Some((0, 128, 0, 1.)),
        "orange" => Some((255, 165, 0, 1.)),
        "yellow" => Some((255, 255, 0, 1.)),
        "purple" => Some((128, 0, 128, 1.)),
        "rebeccapurple" => Some((102, 51, 153, 1.)),
        "gray" | "grey" => Some((128, 128, 128, 1.)),
        "silver" => Some((192, 192, 192, 1.)),
        "maroon" => Some((128, 0, 0, 1.)),
        "olive" => Some((128, 128, 0, 1.)),
        "navy" => Some((0, 0, 128, 1.)),
        "teal" => Some((0, 128, 128, 1.)),
        "aqua" | "cyan" => Some((0, 255, 255, 1.)),
        "fuchsia" | "magenta" => Some((255, 0, 255, 1.)),
        _ => None,
    };
    if let Some((r, g, b, a)) = named {
        return Some(rgb(
            f32::from(r) / 255.,
            f32::from(g) / 255.,
            f32::from(b) / 255.,
            a,
        ));
    }
    let (name, body) = value.split_once('(')?;
    if name != name.trim() {
        return None;
    }
    let body = body.strip_suffix(')')?;
    let parts = if body.contains(',') {
        if body.contains('/') {
            return None;
        }
        let parts = body.split(',').map(str::trim).collect::<Vec<_>>();
        if parts
            .iter()
            .any(|part| part.is_empty() || part.split_whitespace().count() != 1)
        {
            return None;
        }
        parts
    } else {
        let mut split = body.split('/');
        let values = split.next()?.split_whitespace().collect::<Vec<_>>();
        if values.len() != 3 {
            return None;
        }
        let mut parts = values;
        if let Some(alpha) = split.next() {
            let alpha = alpha.trim();
            if alpha.split_whitespace().count() != 1 {
                return None;
            }
            parts.push(alpha);
        }
        if split.next().is_some() {
            return None;
        }
        parts
    };
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let alpha = if parts.len() == 4 {
        unit(parts[3], 1.)?
    } else {
        1.
    };
    match name.trim() {
        "rgb" | "rgba" => Some(rgb(
            unit(parts[0], 255.)?,
            unit(parts[1], 255.)?,
            unit(parts[2], 255.)?,
            alpha,
        )),
        "hsl" | "hsla" => Some(hsl(
            hue(parts[0])?,
            number(parts[1].strip_suffix('%')?)? / 100.,
            number(parts[2].strip_suffix('%')?)? / 100.,
            alpha,
        )),
        _ => None,
    }
}
