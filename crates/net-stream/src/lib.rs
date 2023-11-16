#![deny(rust_2018_idioms, nonstandard_style, future_incompatible)]
#![deny(clippy::mod_module_files)]
#![warn(missing_docs)]
// Enable when the RFC is implemented, see <https://github.com/rust-lang/rust/issues/44663>
// #![feature(public_private_dependencies)]
// in the meantime use https://github.com/awslabs/cargo-check-external-types

//! Server/client networking abstraction library with framing and serialization built-in.

pub mod io_actors;

/// Serialize
pub fn serialize<T: serde::Serialize>(t: &T) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(t)
}

/// Deserialize
pub fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, bincode::Error> {
    bincode::deserialize(&bytes)
}
