#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self { Self { r, g, b, a } }
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }
    pub const BLACK: Self = Self::rgba(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::rgba(1.0, 1.0, 1.0, 1.0);
}
