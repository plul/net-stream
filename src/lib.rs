//! Server/client TCP+UDP networking abstraction library with framing and
//! serialization built-in.

#![deny(clippy::mod_module_files)]
#![warn(missing_docs)]
#![feature(type_alias_impl_trait)]

// Enable when the RFC is implemented, see <https://github.com/rust-lang/rust/issues/44663>
// #![feature(public_private_dependencies)]

pub mod client;
pub mod server;

pub use message_types::MessageTypes;

mod actors;
mod message_types;
mod networking {
    pub(crate) mod tcp;
    pub(crate) mod udp;
}
mod stream_ext;
