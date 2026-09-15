//! The ontologic engine's API, shared by the engine, the CLI and the bench.
//!
//! - [`pb`]: the gRPC messages and service, generated from `proto/ontologic.proto`.
//! - [`client`] (feature `client`): a client that signs in on every call.
//! - [`fake`] (feature `fake`): a fake engine with scripted replies, for tests of clients.

/// The messages and the `Ontologic` service.
pub mod pb {
    // The import stream's events differ in size (a part's `Linked` carries far more than
    // `Finished`); they are sent one at a time, so boxing them would buy nothing.
    #![allow(clippy::large_enum_variant)]
    tonic::include_proto!("ontologic.v1");
}

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "fake")]
pub mod fake;

/// The engine's port when a host names none.
pub const DEFAULT_PORT: u16 = 6969;

/// The metadata every call signs in with: a user name and that user's key.
pub const USER_HEADER: &str = "user";
pub const KEY_HEADER: &str = "key";

/// What the engine answers when `user` and `key` do not belong to one user.
pub const SIGN_IN_FAILED: &str = "sign in failed: wrong user or key";
