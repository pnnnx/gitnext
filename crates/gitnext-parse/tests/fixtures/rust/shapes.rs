//! Geometry primitives used by the renderer.

use std::collections::HashMap;
use std::fmt::{self, Display};

/// Anything with an area.
pub trait Shape: Display {
    /// Area in square units.
    fn area(&self) -> f64;
    /// Default: shapes are drawn filled.
    fn filled(&self) -> bool {
        true
    }
}

/// A circle by radius.
#[derive(Debug, Clone)]
pub struct Circle {
    pub radius: f64,
    label: Option<String>,
}

/// Rectangle kinds.
pub enum Rect {
    Square(f64),
    Box { w: f64, h: f64 },
}

pub type Registry = HashMap<String, Box<dyn Shape>>;

pub const PI2: f64 = std::f64::consts::PI * 2.0;

impl Circle {
    /// New circle with no label.
    pub fn new(radius: f64) -> Self {
        Circle { radius, label: None }
    }

    /// Builder: attach a label.
    pub fn labeled(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }

    fn circumference(&self) -> f64 {
        PI2 * self.radius
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.radius * self.radius
    }
}

impl Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "circle r={} c={}", self.radius, self.circumference())
    }
}

impl Shape for Rect {
    fn area(&self) -> f64 {
        match self {
            Rect::Square(s) => s * s,
            Rect::Box { w, h } => w * h,
        }
    }
    fn filled(&self) -> bool {
        false
    }
}

impl Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rect {}", self.area())
    }
}

/// Sum of areas of every shape in the registry.
pub fn total_area(reg: &Registry) -> f64 {
    reg.values().map(|s| s.area()).sum()
}

/// Largest shape, if any.
pub fn largest<'a, S: Shape + ?Sized>(shapes: &'a [Box<S>]) -> Option<&'a S> {
    shapes.iter().map(|b| b.as_ref()).max_by(|a, b| a.area().total_cmp(&b.area()))
}

pub mod io {
    use super::Circle;

    /// Parse `circle 2.5`.
    pub fn parse(line: &str) -> Option<Circle> {
        let r: f64 = line.strip_prefix("circle ")?.parse().ok()?;
        Some(Circle::new(r))
    }
}

pub async fn load_remote(url: &str) -> Result<Vec<Circle>, String> {
    let _ = url;
    Ok(vec![io::parse("circle 1").unwrap()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_area() {
        assert!(Circle::new(1.0).area() > 3.0);
    }
}
