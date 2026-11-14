//! Colours. The app specifies sRGB hex strings (`"#2e8bff"`); the renderer shades in linear
//! light, as three.js does with its default colour management (`THREE.Color` stores linear
//! values, `lerp` mixes them linearly), and writes to an sRGB target.

/// A linear-light RGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub [f32; 3]);

impl Rgb {
    pub const WHITE: Rgb = Rgb([1.0, 1.0, 1.0]);
    pub const BLACK: Rgb = Rgb([0.0, 0.0, 0.0]);

    /// From `#rrggbb` (or `rrggbb`, `#rgb`). Invalid text gives magenta, so a typo is visible.
    pub fn hex(s: &str) -> Rgb {
        Rgb::from_srgb8(parse_hex(s).unwrap_or([255, 0, 255]))
    }

    pub fn from_srgb8(c: [u8; 3]) -> Rgb {
        Rgb(c.map(|v| srgb_to_linear(v as f32 / 255.0)))
    }

    /// sRGB-encoded components in 0..=1 (for interpolation the way CSS gradients do it).
    pub fn to_srgb(self) -> [f32; 3] {
        self.0.map(linear_to_srgb)
    }

    pub fn from_srgb(c: [f32; 3]) -> Rgb {
        Rgb(c.map(srgb_to_linear))
    }

    /// `self + (other − self) · t`, in linear light (`THREE.Color.lerp`).
    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let [a, b] = [self.0, other.0];
        Rgb([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t))
    }

    pub fn scale(self, k: f32) -> Rgb {
        Rgb(self.0.map(|v| v * k))
    }

    pub fn with_alpha(self, a: f32) -> [f32; 4] {
        [self.0[0], self.0[1], self.0[2], a]
    }
}

pub(crate) fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    let digit = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok();
    match h.len() {
        6 => {
            let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
            Some([byte(0)?, byte(2)?, byte(4)?])
        }
        3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
        _ => None,
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_linear() {
        for s in ["#2e8bff", "#ff9a1f", "#1a1a1a", "#c9ccd1", "#ffffff", "#000000"] {
            let c = parse_hex(s).unwrap();
            let back = Rgb::hex(s).to_srgb().map(|v| (v * 255.0).round() as u8);
            assert_eq!(back, c, "{s}");
        }
        assert_eq!(parse_hex("#abc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_hex("nope"), None);
        assert_eq!(Rgb::hex("#zzzzzz"), Rgb::from_srgb8([255, 0, 255]));
    }

    #[test]
    fn lerp_is_linear() {
        let a = Rgb::hex("#000000");
        let b = Rgb::hex("#ffffff");
        assert_eq!(a.lerp(b, 0.5).0, [0.5; 3]);
    }
}
