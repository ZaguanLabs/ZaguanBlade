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

/// Generic helper: its own type parameter `T` must NOT become a `uses_type`
/// edge, while the real `Point` parameter type must. `Vec`/`Option` are builtin
/// containers (dropped) and the closure trait `Fn` is low-signal noise (dropped),
/// but the closure's argument/return types are kept.
pub fn pick<T>(items: Vec<T>, anchor: Point, choose: &dyn Fn(Color) -> T) -> Option<T> {
    let _ = (items, anchor, choose);
    None
}
