//! The engine's API: generated from `proto/brain.proto`.

// The import stream's events differ in size (a part's `Linked` carries far more than
// `Finished`); they are sent one at a time, so boxing them would buy nothing.
#![allow(clippy::large_enum_variant)]

tonic::include_proto!("brain.v1");
