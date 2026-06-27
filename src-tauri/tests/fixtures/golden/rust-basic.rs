//! Module-level documentation for the geometry fixture.
use std::collections::HashMap;

/// A point in 2D space.
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Color options for rendering.
pub enum Color {
    Red,
    Green,
    Blue,
}

impl Point {
    /// Create a new point.
    pub fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    /// Compute the sum of the coordinates.
    pub fn sum(&self) -> i32 {
        self.x + self.y
    }
}

/// Entry-point helper that builds a point and logs it.
pub fn run(map: HashMap<String, i32>) -> i32 {
    let _ = map;
    let p = Point::new(1, 2);
    println!("sum = {}", p.sum());
    p.sum()
}
