#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self { Self { x, y } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self { Self { width, height } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self { Self { x, y, width, height } }
    pub fn intersection(self, other: Self) -> Self {
        let x=self.x.max(other.x); let y=self.y.max(other.y);
        Self::new(x,y,(self.max_x().min(other.max_x())-x).max(0.0),
            (self.max_y().min(other.max_y())-y).max(0.0))
    }
    pub fn is_empty(self) -> bool { !self.x.is_finite() || !self.y.is_finite()
        || !self.width.is_finite() || !self.height.is_finite() || self.width<=0.0 || self.height<=0.0 }
    pub fn max_x(&self) -> f32 { self.x + self.width }
    pub fn max_y(&self) -> f32 { self.y + self.height }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2D {
    pub sx: f32,
    pub kx: f32,
    pub tx: f32,
    pub ky: f32,
    pub sy: f32,
    pub ty: f32,
}

impl Affine2D {
    pub const IDENTITY: Self = Self { sx: 1.0, kx: 0.0, tx: 0.0, ky: 0.0, sy: 1.0, ty: 0.0 };

    pub fn translation(x: f32, y: f32) -> Self {
        Self { tx: x, ty: y, ..Self::IDENTITY }
    }

    pub fn scale(x: f32, y: f32) -> Self {
        Self { sx: x, sy: y, ..Self::IDENTITY }
    }

    pub fn map_point(&self, p: Point) -> Point {
        Point {
            x: self.sx * p.x + self.kx * p.y + self.tx,
            y: self.ky * p.x + self.sy * p.y + self.ty,
        }
    }

    pub fn then(&self, other: Affine2D) -> Self {
        Self {
            sx: other.sx * self.sx + other.kx * self.ky,
            kx: other.sx * self.kx + other.kx * self.sy,
            tx: other.sx * self.tx + other.kx * self.ty + other.tx,
            ky: other.ky * self.sx + other.sy * self.ky,
            sy: other.ky * self.kx + other.sy * self.sy,
            ty: other.ky * self.tx + other.sy * self.ty + other.ty,
        }
    }
}
