//! Protobuf types for Cadpole, generated from `proto/cadpole/v1/*.proto`.
//!
//! The generated crate is `cadpole_v1_proto` (built by `cadpole_proto_library` in
//! `build_support/proto/defs.bzl`); this wrapper re-exports it under a stable
//! path and re-exports `prost` so callers get the same `Message` trait the
//! generated code was built against.
//!
//! [`descriptor`] is the same schema as data: the serialized
//! `FileDescriptorSet`, with comments, that `cadpole-ops` reflects over to build
//! every front-end.

pub use cadpole_v1_descriptor::FILE_DESCRIPTOR_SET as descriptor;
pub use cadpole_v1_proto::cadpole::v1;
pub use prost;
pub use prost_types;

/// Cadpole's two file types. The values are set once, in
/// `build_support/file_types.bzl`; nothing else spells them out.
pub mod file_types {
    /// A packed document: a serialised `cadpole.v1.Document`.
    pub const DOCUMENT_EXT: &str = env!("CADPOLE_DOCUMENT_EXT");
    /// The document's uniform type identifier (the Apple apps declare it).
    pub const DOCUMENT_UTTYPE: &str = env!("CADPOLE_DOCUMENT_UTTYPE");
    /// A baked model: an evaluated `BakedModel`, zlib-deflated.
    pub const BAKED_EXT: &str = env!("CADPOLE_BAKED_EXT");
}

/// Well-known types as generated for *our* descriptors.
///
/// `rules_rust_prost` compiles `google/protobuf/*.proto` into their own crates
/// rather than mapping them onto `prost-types`, so `v1::Event::data` is a
/// [`wkt::Struct`], not a `prost_types::Struct`. Code that builds those values
/// must use this path.
pub mod wkt {
    pub use cadpole_v1_proto::struct_proto::google::protobuf::{ListValue, NullValue, Struct, Value, value};
    pub use cadpole_v1_proto::timestamp_proto::google::protobuf::Timestamp;
}
