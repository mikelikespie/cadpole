//! Cadpole core: the feature tree, sketches, the two kernel bindings (truck for
//! B-rep, manifold for meshes),
//! tessellation and headless rendering.
//!
//! This crate knows nothing about Starlark. `cadpole-script` drives it through
//! [`Context`], and `cadpole-engine` turns the results into protobuf.

pub mod animation;
pub mod assembly;
pub mod brepmesh;
pub mod cdt;
pub mod chamfer;
pub mod clearance;
pub mod contact;
pub mod context;
pub mod curve;
pub mod emboss;
pub mod export;
pub mod fill;
pub mod fillet;
pub mod fmath;
pub mod geom;
pub mod gridjoin;
pub mod heightfield;
pub mod ids;
pub mod kernel;
pub mod ksurface;
pub mod layers;
pub mod loft;
pub mod memo;
pub mod mesh_clean;
pub mod mesh_kernel;
pub mod naming;
pub mod opt;
pub mod paint;
pub mod params;
pub mod pattern;
pub mod primitives;
pub mod render;
pub mod select;
pub mod shadow;
pub mod sketch;
pub mod sketch2d;
pub mod skin;
pub mod solid;
pub mod sun;
pub mod surface_class;
pub mod sweep;
pub mod tangent;
pub mod terrain;
pub mod tessellate;
pub mod time;

pub use assembly::{AssemblyId, Frame, InstanceId, PartId};
pub use chamfer::{Chamfered, EdgeChamfer, Setback, SetbackCurve};
pub use fillet::Filleted;
pub use context::{Body, BodyRecord, Context, Feature, Span, SplitTool};
pub use geom::{Plane, Transform, Vec3};
pub use ids::{BodyId, FeatureId};
pub use params::{ParamDecl, ParamValue};
pub use fill::FillRule;
pub use curve::{Curve, Segment3};
pub use emboss::{Embossed, Wrapped};
pub use sketch::{Loop, Path, Segment, Sketch};
pub use surface_class::SurfaceClass;

pub use cadpole_proto::v1 as proto;

/// Errors raised by modeling operations. Scripts see these as diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum CadpoleError {
    #[error("{0}")]
    Invalid(String),
    #[error("kernel operation failed: {0}")]
    Kernel(String),
    #[error("unknown body {0}")]
    UnknownBody(BodyId),
    #[error("unknown feature {0}")]
    UnknownFeature(String),
}

pub type Result<T> = std::result::Result<T, CadpoleError>;

impl CadpoleError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        CadpoleError::Invalid(msg.into())
    }
    pub fn kernel(msg: impl Into<String>) -> Self {
        CadpoleError::Kernel(msg.into())
    }
}
