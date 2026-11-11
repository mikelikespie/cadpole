//! Cadpole 3D import.
//!
//! Two readers behind one posture. **STEP** ([`step`]) parses ISO 10303-21
//! text with `truck-stepio`'s lexer and typed holder table, then walks every
//! `CLOSED_SHELL` itself — the crate's own `to_compressed_shell` drops the
//! faces it cannot convert and the names it parsed — converting each surface
//! and curve to the `truck_modeling` types every extrude and boolean in Cadpole
//! already produces, so an imported body *is* a `KSolid`: a boolean operand,
//! STEP-exportable, with exact normals and bound-driven tessellation
//! and `Face.kind` from `SurfaceClass`. **Meshes**
//! ([`mesh`]) read binary and ASCII STL, OBJ and 3MF — a standard package or
//! a Bambu Studio project — into mesh bodies with a [`mesh::Report`] that
//! measures rather than mends: `solid::validate`'s verdict, folds,
//! the bounding box, the volume when it is closed, and the chord error it
//! cannot measure. **DXF** ([`dxf`]) is the one 2D reader
//! and Cadpole's own: an ASCII DXF's group-code/value pairs into a
//! [`dxf::Drawing`] of layers, entities in millimetres, labels and points
//! — a surveyor's site plan — that a script reads layer by layer.
//!
//! **Untrusted input** (docs/security.md, "Imports"). Every reader runs
//! after a byte-level [`prescan`] that refuses what would hang or overflow a
//! parser, under [`bounded::run`] — a thread with a 64 MB stack and a
//! deadline, so a hostile file cannot hold an evaluation — and every panic
//! inside a third-party parser comes back as a diagnostic. Nothing here
//! prints.

pub mod bounded;
pub mod dxf;
pub mod gltf;
pub mod label;
pub mod mesh;
pub mod obj;
pub mod prescan;
pub mod step;
pub mod stl;
pub mod threemf;

#[cfg(test)]
mod gltf_tests;
#[cfg(test)]
mod regress_tests;
#[cfg(test)]
mod tests;

/// Bumped whenever a reader's output for the same bytes could change, so the
/// memo and the parsed-result cache never serve a body an older
/// importer made.
pub const IMPORTER_VERSION: u32 = 3;

/// How loud a note is. An import that *fails* is an `Err` at the call; a
/// note is something the caller should know about a body it did get.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
}

/// One thing a reader wants said about what it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub level: Level,
    pub message: String,
}

impl Note {
    pub fn info(message: impl Into<String>) -> Note {
        Note { level: Level::Info, message: message.into() }
    }
    pub fn warning(message: impl Into<String>) -> Note {
        Note { level: Level::Warning, message: message.into() }
    }
}

/// The wall-clock budget one import may spend parsing and converting, in
/// milliseconds, when the evaluation's own deadline is further away than
/// this (docs/security.md). A 1 MB vendor STEP converts in well under a
/// second; a file that needs a minute is not one to wait for.
pub const IMPORT_BUDGET_MS: u64 = 60_000;
