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

/// Reads the `API_KEY` environment variable (M4.2 `reads_env` fixture). The
/// `std::env::var` accessor also yields the usual `call` edge to `var`.
pub fn load_api_key() {
    let _ = std::env::var("API_KEY");
}

/// Holds a field named `env` whose `var` method shadows the env accessor name.
struct EnvReader {
    env: Point,
}

impl EnvReader {
    /// NEGATIVE `reads_env` fixture (M4.2 anchoring): `self.env.var(...)` is a
    /// FIELD access (a `field_expression` callee), NOT the `std::env::var` PATH,
    /// so it must NOT emit a `reads_env` edge — only the usual `call` edge to
    /// `var`.
    fn read_field(&self) {
        let _ = self.env.var("NOT_ENV");
    }
}
