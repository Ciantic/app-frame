//! Database building blocks shared by the service crates.
//!
//! Currently only [`Patch`], the update input that tells "not given" apart from
//! "set to `NULL`". Everything else is designed one piece at a time.

mod patch;

pub use patch::Patch;
