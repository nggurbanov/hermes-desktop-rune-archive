//! Hermes' stored shape vocabulary, rendered through Rune's native SVG image cache.
use super::*;
use std::sync::Arc;
const KINDS: &[&str] = &[
    "round", "organic", "boxy", "capsule", "nub", "cloud", "droplet", "hexagon", "sun", "triangle",
];
pub fn blob_parts(shape: &str) -> (&str, &str) {
    let mut p = shape.split(':');
    p.next();
    let seed = p.next().unwrap_or("");
    let kind = p.next().unwrap_or("");
    (seed, if KINDS.contains(&kind) { kind } else { "" })
}
pub fn blob_shape(seed: &str, kind: &str) -> String {
    if !kind.is_empty() {
        format!("blobatar:{seed}:{kind}")
    } else if !seed.is_empty() {
        format!("blobatar:{seed}")
    } else {
        "blobatar".into()
    }
}
pub fn valid_color(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|c| c.is_ascii_hexdigit());
    }
    let Some(v) = value.strip_prefix("hsl(").and_then(|s| s.strip_suffix(')')) else {
        return false;
    };
    let parts = v.split_whitespace().collect::<Vec<_>>();
    parts.len() == 3
        && parts[0]
            .parse::<f32>()
            .is_ok_and(|n| n.is_finite() && (0.0..=360.0).contains(&n))
        && parts[1..].iter().all(|s| {
            s.strip_suffix('%')
                .and_then(|n| n.parse::<f32>().ok())
                .is_some_and(|n| n.is_finite() && (0.0..=100.0).contains(&n))
        })
}
fn hash(value: &str) -> u32 {
    value
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32))
}
pub fn face_svg(shape: &str, color: &str, name: &str) -> String {
    let mut shape = shape;
    let owned: String;
    let mut seed = name;
    if shape == "blob" || shape.starts_with("blobatar") {
        let (locked, kind) = blob_parts(shape);
        if !locked.is_empty() {
            seed = locked;
        }
        shape = match kind {
            "round" => "circle",
            "boxy" => "squircle",
            "capsule" => "pill",
            "cloud" => "cloud",
            "droplet" => "drop",
            "hexagon" => "hexagon",
            "triangle" => "triangle",
            "organic" => "blob",
            "nub" => "nub",
            "sun" => "sun",
            _ => [
                "circle", "blob", "squircle", "pill", "triangle", "hexagon", "cloud", "drop",
            ][(hash(seed) % 8) as usize],
        };
    }
    let color = if valid_color(color) && !color.is_empty() {
        color
    } else {
        owned = format!("hsl({} 68% 58%)", hash(seed) % 360);
        &owned
    };
    if let Some(seed) = shape.strip_prefix("sigil-") {
        return sigil_svg(name, seed, color);
    }
    let body = match shape {
        "squircle" => "<rect x='3' y='3' width='34' height='34' rx='11'/>",
        "pill" => "<rect x='2' y='7' width='36' height='26' rx='13'/>",
        "triangle" | "tetrahedron" => {
            "<path d='M20 5.5 L36 33.5 L4 33.5 Z' stroke-width='4' stroke-linejoin='round'/>"
        }
        "hexagon" => {
            "<path d='M20 3.5 L34.5 11.75 L34.5 28.25 L20 36.5 L5.5 28.25 L5.5 11.75 Z' stroke-width='4' stroke-linejoin='round'/>"
        }
        "cloud" => {
            "<path d='M11 32 a7.5 7.5 0 0 1 -1 -14.9 A9.5 9.5 0 0 1 29 12.5 A7 7 0 0 1 30 32 Z'/>"
        }
        "drop" => "<path d='M20 3 C20 3 6 20 6 27 a14 13.5 0 0 0 28 0 C34 20 20 3 20 3 Z'/>",
        "cube" => {
            "<path d='M20 4 L33 11 L33 29 L20 36 L7 29 L7 11 Z'/><path fill='none' stroke='#0006' stroke-width='1.4' d='M7 11 L20 18 L33 11 M20 18 L20 36'/>"
        }
        "octahedron" => {
            "<path d='M20 3 L36 20 L20 37 L4 20 Z'/><path fill='none' stroke='#0006' stroke-width='1.4' d='M4 20 L36 20 M20 3 L20 37'/>"
        }
        "dodecahedron" => {
            "<path d='M20 3 L30 6.2 L36.2 14.7 L36.2 25.3 L30 33.8 L20 37 L10 33.8 L3.8 25.3 L3.8 14.7 L10 6.2 Z'/>"
        }
        "icosahedron" => "<path d='M20 3 L34.7 11.5 L34.7 28.5 L20 37 L5.3 28.5 L5.3 11.5 Z'/>",
        "sun" => {
            "<path d='M20 1 L24 8 L31 4 L31 12 L39 13 L34 20 L39 27 L31 28 L31 36 L24 32 L20 39 L16 32 L9 36 L9 28 L1 27 L6 20 L1 13 L9 12 L9 4 L16 8 Z'/>"
        }
        "nub" => {
            "<path d='M7 12 C4 5 15 0 20 7 C25 0 36 5 33 12 C42 16 37 26 33 27 C35 38 24 40 20 34 C16 40 5 38 7 27 C-2 23 -1 15 7 12Z'/>"
        }
        "blob" => {
            "<path d='M20 3 C33 -1 40 13 36 25 C40 36 28 41 18 36 C5 40 -1 27 5 18 C2 8 9 0 20 3 Z'/>"
        }
        _ => "<circle cx='20' cy='20' r='17.5'/>",
    };
    let eye_y = if shape == "cloud" { 22.0 } else { 17.2 };
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 44'><g fill='{color}' stroke='{color}'>{body}</g><g fill='#252531'><ellipse cx='15.4' cy='{eye_y}' rx='2.2' ry='2.3'/><ellipse cx='24.6' cy='{eye_y}' rx='2.2' ry='2.3'/></g><g fill='#fff' opacity='.85'><circle cx='14.8' cy='{}' r='.65'/><circle cx='24' cy='{}' r='.65'/></g></svg>",
        eye_y - 0.7,
        eye_y - 0.7
    )
}
fn sigil_svg(name: &str, seed: &str, color: &str) -> String {
    let mut state = format!("{name}::{seed}")
        .encode_utf16()
        .fold(2166136261u32, |h, c| {
            (h ^ (c as u32)).wrapping_mul(16777619)
        });
    if state == 0 {
        state = 88675123;
    }
    let mut rng = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f64 / 4294967296.0
    };
    let count = 4 + (rng() * 3.0).floor() as i32;
    let mut strokes = String::new();
    for _ in 0..count {
        let x = (rng() * 3.0).floor() as i32;
        let y = (rng() * 5.0).floor() as i32;
        let x2 = (x + if rng() > 0.5 { 1 } else { -1 }).clamp(0, 2);
        let y2 = (y + (rng() * 3.0).floor() as i32 - 1).clamp(0, 4);
        strokes.push_str(&format!(
            "M{} {} L{} {} M{} {} L{} {} ",
            6 + x * 7,
            8 + y * 6,
            6 + x2 * 7,
            8 + y2 * 6,
            6 + (4 - x) * 7,
            8 + y * 6,
            6 + (4 - x2) * 7,
            8 + y2 * 6
        ));
        if rng() > 0.6 {
            strokes.push_str(&format!(
                "M{} {} L{} {} ",
                6 + x2 * 7,
                8 + y2 * 6,
                6 + (4 - x2) * 7,
                8 + y2 * 6
            ));
        }
    }
    strokes.push_str("M20 8 L20 32");
    let ring = if rng() > 0.45 {
        "<path d='M20 4 L36 20 L20 36 L4 20 Z' opacity='.5' stroke-width='1.2'/>"
    } else {
        ""
    };
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 44'><g fill='none' stroke='{color}' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'>{ring}<path d='{strokes}'/></g><g fill='{color}'><circle cx='16' cy='14' r='2.4'/><circle cx='24' cy='14' r='2.4'/></g></svg>"
    )
}
impl Bots {
    pub fn shape_preview(&self, size: f32, scale: f32) -> Option<Image> {
        let name = if self.name.text.is_empty() {
            "agent"
        } else {
            &self.name.text
        };
        self.images.as_ref().map(|c| {
            c.svg(
                Arc::<[u8]>::from(face_svg(&self.shape.text, &self.color.text, name).into_bytes()),
                size,
                scale,
            )
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_blob_seed_and_kind_roundtrip() {
        assert_eq!(blob_parts("blobatar::cloud"), ("", "cloud"));
        assert_eq!(blob_shape("locked", "triangle"), "blobatar:locked:triangle");
        assert!(valid_color("hsl(120 68% 58%)"));
        assert!(!valid_color("red' onclick='bad"));
        assert!(!valid_color("hsl(NaN 12% 14%)"));
    }
}
