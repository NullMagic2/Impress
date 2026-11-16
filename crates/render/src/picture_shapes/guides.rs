//! Bounded DrawingML guide formulas and built-in shape coordinates.
use std::collections::BTreeMap;
const ANGLE_TO_RAD: f64 = std::f64::consts::PI / 10_800_000.0;
const FULL_TURN: f64 = 21_600_000.0;
/// Reject extreme guide values before they enter path or raster arithmetic.
const MAX_GUIDE_MAGNITUDE: f64 = 1.0e100;

pub(super) struct Values { width: f64, height: f64, assigned: BTreeMap<String, f64> }
impl Values {
    pub fn new(width: f64, height: f64) -> Self { Self { width, height, assigned: BTreeMap::new() } }
    pub fn get(&self, name: &str) -> Option<f64> {
        let (w, h) = (self.width, self.height);
        let value = if let Ok(value) = name.parse::<f64>() { value }
        else if let Some(value) = self.assigned.get(name) { *value }
        else {
            match name {
                "w" | "r" => w, "h" | "b" => h, "l" | "t" => 0.0,
                "hc" => w / 2.0, "vc" => h / 2.0, "ss" => w.min(h), "ls" => w.max(h),
                _ if name.starts_with("wd") => w / name[2..].parse::<f64>().ok()?,
                _ if name.starts_with("hd") => h / name[2..].parse::<f64>().ok()?,
                _ if name.starts_with("ssd") => w.min(h) / name[3..].parse::<f64>().ok()?,
                _ if name.contains("cd") => {
                    let (factor, divisor) = name.split_once("cd")?;
                    factor.parse::<f64>().unwrap_or(1.0) * FULL_TURN / divisor.parse::<f64>().ok()?
                }
                _ => return None,
            }
        };
        (value.is_finite() && value.abs() <= MAX_GUIDE_MAGNITUDE).then_some(value)
    }
    pub fn assign(&mut self, name: &str, formula: &str) -> Option<()> {
        let mut parts = formula.split_ascii_whitespace();
        let operation = parts.next()?;
        let arguments = parts.map(|p| self.get(p)).collect::<Option<Vec<_>>>()?;
        if arguments.len() > 3 { return None; }
        let x = *arguments.first()?;
        let y = *arguments.get(1).unwrap_or(&0.0);
        let z = *arguments.get(2).unwrap_or(&0.0);
        let divide = |a, b| if b == 0.0 { 0.0 } else { a / b };
        let value = match operation {
            "val" => x, "+-" => x + y - z, "*/" => divide(x * y, z), "+/" => divide(x + y, z),
            "?:" => if x > 0.0 { y } else { z }, "abs" => x.abs(),
            "at2" => y.atan2(x) / ANGLE_TO_RAD,
            "cat2" => x * z.atan2(y).cos(), "sat2" => x * z.atan2(y).sin(),
            "cos" => x * (y * ANGLE_TO_RAD).cos(), "sin" => x * (y * ANGLE_TO_RAD).sin(),
            "tan" => x * (y * ANGLE_TO_RAD).tan(), "max" => x.max(y), "min" => x.min(y),
            "mod" => (x * x + y * y + z * z).sqrt(), "pin" => y.max(x).min(z),
            "sqrt" => x.max(0.0).sqrt(), _ => return None,
        };
        if !value.is_finite() || value.abs() > MAX_GUIDE_MAGNITUDE { return None; }
        self.assigned.insert(name.into(), value);
        Some(())
    }
}
