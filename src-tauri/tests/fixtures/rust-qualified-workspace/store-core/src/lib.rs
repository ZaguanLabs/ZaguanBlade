pub mod store;
pub mod other;
pub mod cycle_a;
pub mod cycle_b;
mod impls;

pub use store::Store as PublicStore;

pub trait Maker {
    fn make() -> Self;
}
