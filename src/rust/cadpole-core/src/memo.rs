//! The kernel-op memo: incremental re-evaluation without touching
//! the language.
//!
//! A script is re-run from the top on every change — Starlark is a percent
//! of the cost — and one cache sits in front of the kernel. Every call that
//! makes a body (`Context::{extrude, revolve, loft, cone, pattern, boolean,
//! glue, split, mirror, scale, sphere, torus, hull}`) and every display
//! tessellation (`Context::body_mesh`) is keyed by a canonical byte string
//! of everything that decides its answer, and the entry stores everything
//! the op produced, replayed on a hit.
//!
//! **The key** ([`KeyBuilder`], [`OpKey`]) is bytes, not a digest: the
//! kernel version, the op kind, the feature id the names are minted under,
//! the operands' identities, every numeric input as its `f64` bit pattern,
//! the routing inputs that decide brep/mesh (the kernel choice, the face
//! budget, the mesh engine, the snap switch) and the tolerances. A hit is
//! decided by comparing those bytes in full — the store is a map keyed by
//! them — so no collision argument is needed and there is no tolerant
//! comparison anywhere: a slider step of one ulp is a different key, because
//! the kernel's answer is a discontinuous function of its inputs.
//!
//! **A body's identity** ([`BodyKey`]) is the digest of the key of the op
//! that produced it, plus its index among that op's outputs — a Merkle
//! chain, so a body's identity is the hash of everything that made it. A
//! body whose producer did not key it is [`BodyKey::Unkeyed`], and an op
//! with an unkeyed operand is never looked up or stored: that is the safe
//! default for a producer the memo does not know about.
//!
//! **The value** ([`Produced`]) is what the op produced: its bodies with
//! their [`NameSource`]s (so a hit names identically), a realised mesh where
//! the op handed one in, the notes and routing diagnostics it emitted, in
//! order, and the feature args it set after the fact. Tessellations are
//! their own entries keyed by (body key, tessellation options).
//!
//! **Where it lives** ([`Memo`]): outside `Context`. `cadpole serve`'s session
//! and the wasm `Engine` hold one for their lifetime; `cadpole test` shares one
//! across sweep cells; `cadpole eval` passes none and pays nothing. Bounded by
//! bytes, evicted least-recently-used, per process (never persisted, never
//! shared between engines), and tagged with [`KERNEL_VERSION`] so an
//! entry from another kernel is unreachable.
//!
//! Everything here is integer arithmetic: the digest is SHA-256 written out
//! below (no new dependency), so a key is byte-identical on every target.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use crate::context::{Body, NameSource, Severity};
use crate::curve::{Curve, Segment3};
use crate::geom::{Plane, Transform, Vec3};
use crate::ids::FeatureId;
use crate::kernel::{BoolOp, ChordError, KSolid, KernelChoice, TessellationOptions};
use crate::loft::{Cap, Section, SectionMatch};
use crate::mesh_kernel::{self, MeshBody};
use crate::sketch::{Loop, P2, Path, Segment, Sketch};
use crate::tessellate::{Mesh, Topology};

/// Bumped whenever an op's answer for the same key can change: a kernel
/// upgrade, a naming change, a tessellation change. It is the first thing
/// in every key, so an entry made under another version can never be hit,
/// and [`Memo::attach`] clears a store made under another version outright.
pub const KERNEL_VERSION: u32 = 1;

/// The default bound: 256 MB natively. The compressor wheel's intermediate
/// bodies sum to ~25 MB, so a whole drag history fits many times over.
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_LIMIT_BYTES: usize = 256 << 20;
/// 96 MB in wasm, where memory is 32-bit and the module itself is 20 MB.
#[cfg(target_arch = "wasm32")]
pub const DEFAULT_LIMIT_BYTES: usize = 96 << 20;

// ---------------------------------------------------------------------------
// SHA-256, so a body's identity is the same bytes on every target
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// SHA-256 of `data` (FIPS 180-4), as the 32 bytes of the digest.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    let mut w = [0u32; 64];
    for block in padded.chunks_exact(64) {
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A body's identity: the digest of the key of the op that produced it and
/// its index among that op's outputs, or nothing when the producer did not
/// key it. Compared as bytes; printed as hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BodyKey {
    Keyed([u8; 32]),
    /// The producer said nothing about where this body came from (a producer
    /// the memo does not know about), so nothing downstream of it can be
    /// served from the memo.
    Unkeyed,
}

impl BodyKey {
    /// The identity of a body whose producer keys it by its *content*: a
    /// mesh handed to the context ready-made (a field contour, an import).
    /// Hashing the triangles is the cost, so callers do it only when a memo
    /// is attached.
    pub fn of_mesh(mesh: &Mesh, topo: &Topology) -> BodyKey {
        let mut k = KeyBuilder::new("content");
        k.tag("positions");
        for p in &mesh.positions {
            k.buf.extend_from_slice(&p.to_bits().to_le_bytes());
        }
        k.tag("indices");
        for i in &mesh.indices {
            k.buf.extend_from_slice(&i.to_le_bytes());
        }
        k.tag("face_ids");
        for i in &mesh.face_ids {
            k.buf.extend_from_slice(&i.to_le_bytes());
        }
        k.tag("faces");
        for f in &topo.faces {
            k.str(&f.name);
            k.u64(f.aliases.len() as u64);
            for a in &f.aliases {
                k.str(a);
            }
        }
        BodyKey::Keyed(sha256(&k.buf))
    }

    pub fn is_keyed(&self) -> bool {
        matches!(self, BodyKey::Keyed(_))
    }

    /// Lowercase hex of the digest, or `unkeyed`.
    pub fn hex(&self) -> String {
        match self {
            BodyKey::Keyed(d) => d.iter().map(|b| format!("{b:02x}")).collect(),
            BodyKey::Unkeyed => "unkeyed".to_owned(),
        }
    }
}

/// The canonical bytes of one kernel op's inputs, and whether every operand
/// was keyed (an unkeyed operand makes the op unmemoisable).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct OpKey {
    bytes: Vec<u8>,
    keyed: bool,
}

impl OpKey {
    /// False when an operand was [`BodyKey::Unkeyed`]: the op runs, and
    /// nothing is looked up or stored.
    pub fn is_memoisable(&self) -> bool {
        self.keyed
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The identity of the `index`th body this op produces.
    pub fn body_key(&self, index: usize) -> BodyKey {
        if !self.keyed {
            return BodyKey::Unkeyed;
        }
        let mut material = self.bytes.clone();
        material.extend_from_slice(b"\x00output");
        material.extend_from_slice(&(index as u64).to_le_bytes());
        BodyKey::Keyed(sha256(&material))
    }
}

/// Writes the canonical bytes of an op's inputs. Every value goes in under
/// a one-byte type tag and a fixed width, so two different inputs cannot
/// serialise to the same bytes; numbers are their bit patterns, never a
/// rounding of them.
pub struct KeyBuilder {
    buf: Vec<u8>,
    keyed: bool,
}

impl KeyBuilder {
    /// Start a key for the op called `op`. The kernel version goes first.
    pub fn new(op: &str) -> KeyBuilder {
        let mut k = KeyBuilder { buf: Vec::with_capacity(256), keyed: true };
        k.buf.push(b'K');
        k.buf.extend_from_slice(&KERNEL_VERSION.to_le_bytes());
        k.tag(op);
        k
    }

    fn raw_str(&mut self, tag: u8, s: &str) {
        self.buf.push(tag);
        self.buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// A structural label (an op or field name).
    pub fn tag(&mut self, s: &str) -> &mut Self {
        self.raw_str(b'T', s);
        self
    }

    /// A string input (a feature id, a loop name, a kernel choice).
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.raw_str(b'S', s);
        self
    }

    /// A number, as its bit pattern.
    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.buf.push(b'F');
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.push(b'U');
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.buf.push(if v { b'1' } else { b'0' });
        self
    }

    /// An operand's identity. An unkeyed operand marks the whole op
    /// unmemoisable.
    pub fn body(&mut self, k: &BodyKey) -> &mut Self {
        match k {
            BodyKey::Keyed(d) => {
                self.buf.push(b'B');
                self.buf.extend_from_slice(d);
            }
            BodyKey::Unkeyed => {
                self.buf.push(b'?');
                self.keyed = false;
            }
        }
        self
    }

    /// Any input with a canonical form.
    pub fn put<T: Canon + ?Sized>(&mut self, v: &T) -> &mut Self {
        v.canon(self);
        self
    }

    pub fn finish(self) -> OpKey {
        OpKey { bytes: self.buf, keyed: self.keyed }
    }
}

/// An input that can write its canonical bytes. Implemented for every type
/// that reaches the kernel from a script, so an op is keyed by handing the
/// builder the values it is about to compute from — never a description of
/// them (`Feature.args` renders a loft as "profile, profile, …").
pub trait Canon {
    fn canon(&self, k: &mut KeyBuilder);
}

impl Canon for f64 {
    fn canon(&self, k: &mut KeyBuilder) {
        k.f64(*self);
    }
}

impl Canon for bool {
    fn canon(&self, k: &mut KeyBuilder) {
        k.bool(*self);
    }
}

impl Canon for str {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self);
    }
}

impl Canon for String {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self);
    }
}

impl Canon for FeatureId {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("feature").str(&self.0);
    }
}

impl Canon for BodyKey {
    fn canon(&self, k: &mut KeyBuilder) {
        k.body(self);
    }
}

impl<T: Canon> Canon for [T] {
    fn canon(&self, k: &mut KeyBuilder) {
        k.buf.push(b'[');
        k.buf.extend_from_slice(&(self.len() as u64).to_le_bytes());
        for v in self {
            v.canon(k);
        }
    }
}

impl<T: Canon> Canon for Vec<T> {
    fn canon(&self, k: &mut KeyBuilder) {
        self.as_slice().canon(k);
    }
}

impl<T: Canon> Canon for Option<T> {
    fn canon(&self, k: &mut KeyBuilder) {
        match self {
            Some(v) => {
                k.buf.push(b'+');
                v.canon(k);
            }
            None => k.buf.push(b'-'),
        }
    }
}

impl<T: Canon, const N: usize> Canon for [T; N] {
    fn canon(&self, k: &mut KeyBuilder) {
        self.as_slice().canon(k);
    }
}

impl Canon for Vec3 {
    fn canon(&self, k: &mut KeyBuilder) {
        k.buf.push(b'v');
        k.f64(self.x).f64(self.y).f64(self.z);
    }
}

impl Canon for P2 {
    fn canon(&self, k: &mut KeyBuilder) {
        k.buf.push(b'p');
        k.f64(self.x).f64(self.y);
    }
}

impl Canon for Plane {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("plane").put(&self.origin).put(&self.normal).put(&self.x_dir);
    }
}

impl Canon for Transform {
    fn canon(&self, k: &mut KeyBuilder) {
        let m: [[f64; 4]; 4] = self.0.into();
        k.tag("transform");
        for col in &m {
            for v in col {
                k.f64(*v);
            }
        }
    }
}

impl Canon for Segment {
    fn canon(&self, k: &mut KeyBuilder) {
        match self {
            Segment::Line { a, b } => {
                k.tag("line").put(a).put(b);
            }
            Segment::Arc { a, b, via } => {
                k.tag("arc").put(a).put(b).put(via);
            }
            Segment::Bezier { a, b, ctrl } => {
                k.tag("bezier").put(a).put(b).put(ctrl);
            }
        }
    }
}

impl Canon for Loop {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("loop").put(&self.segments);
    }
}

impl Canon for Path {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("path").put(&self.start).put(&self.segments);
    }
}

impl Canon for Sketch {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("sketch").put(&self.plane).put(&self.loops).put(&self.loop_names).put(&self.paths).put(&self.path_names);
    }
}

impl Canon for Segment3 {
    fn canon(&self, k: &mut KeyBuilder) {
        match self {
            Segment3::Line { a, b } => {
                k.tag("line3").put(a).put(b);
            }
            Segment3::Arc { a, b, via } => {
                k.tag("arc3").put(a).put(b).put(via);
            }
            Segment3::Bezier { a, b, ctrl } => {
                k.tag("bezier3").put(a).put(b).put(ctrl);
            }
            Segment3::Helix { a, centre, axis, r_end, angle, advance } => {
                k.tag("helix3").put(a).put(centre).put(axis).f64(*r_end).f64(*angle).f64(*advance);
            }
        }
    }
}

impl Canon for Curve {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("curve").put(&self.start).put(&self.segments).bool(self.closed);
    }
}

impl Canon for Section {
    fn canon(&self, k: &mut KeyBuilder) {
        match self {
            Section::Profile(s) => {
                k.tag("profile").put(s);
            }
            Section::Curve(c) => {
                k.tag("curve").put(c);
            }
            Section::Point(p) => {
                k.tag("point").put(p);
            }
        }
    }
}

impl Canon for SectionMatch {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self.name());
    }
}

impl Canon for Cap {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self.name());
    }
}

impl Canon for BoolOp {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self.kind_name());
    }
}

impl Canon for KernelChoice {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self.name());
    }
}

impl Canon for mesh_kernel::Engine {
    fn canon(&self, k: &mut KeyBuilder) {
        k.str(self.name());
    }
}

impl Canon for TessellationOptions {
    fn canon(&self, k: &mut KeyBuilder) {
        k.tag("tessellation").f64(self.tolerance).f64(self.max_normal_degrees);
    }
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// One diagnostic an op emitted while it ran — a kernel note, a routing
/// decision, the unattributed-faces warning — replayed on a hit under the
/// same feature, in the same order.
#[derive(Clone, Debug)]
pub struct Note {
    pub severity: Severity,
    pub message: String,
    pub feature: FeatureId,
    /// Whether it went on the feature's own list as well as the document's
    /// (`Context::info` does both; a tessellation warning only the latter).
    pub on_feature: bool,
}

/// One body an op produced: the geometry, where its names come from, the
/// mesh it was handed ready-made (a mirror or non-uniform scale)
/// and its identity.
#[derive(Clone)]
pub struct ProducedBody {
    pub body: Body,
    pub names: NameSource,
    pub mesh: Option<Arc<MeshBody>>,
    pub key: BodyKey,
}

impl ProducedBody {
    pub fn new(body: Body, names: NameSource) -> ProducedBody {
        ProducedBody { body, names, mesh: None, key: BodyKey::Unkeyed }
    }
}

/// What a display tessellation produced: the named mesh, how many of its
/// faces matched no surface, the tolerance it was made at and the chord
/// error measured (`Context::body_mesh`).
#[derive(Clone)]
pub struct MeshOutcome {
    pub mesh: Arc<MeshBody>,
    pub unmatched: usize,
    pub at: f64,
    pub chord: Option<ChordError>,
}

/// Everything one op produced, so a hit replays it: bodies, a tessellation,
/// the diagnostics in order, the feature args it set, and — for a producer
/// whose answer is not a body — a payload.
#[derive(Clone, Default)]
pub struct Produced {
    pub bodies: Vec<ProducedBody>,
    pub mesh: Option<MeshOutcome>,
    pub notes: Vec<Note>,
    pub args: Vec<(FeatureId, String, String)>,
    /// What an op outside the kernel produced when it is not a body: an
    /// imported 2D drawing that a script reads layer by layer. Typed
    /// by the caller through [`crate::Context::produce_value`]; a hit hands
    /// the same `Arc` back, so nothing is parsed or copied.
    pub payload: Option<Arc<dyn std::any::Any + Send + Sync>>,
    /// The payload's size, estimated by the producer, for the memo's bound.
    pub payload_bytes: usize,
    /// An answer that is not a function of its key and so is returned but
    /// never stored: a solve that a time limit stopped depends on the clock,
    /// and deadline stops are not cached.
    pub transient: bool,
}

impl Produced {
    pub fn bodies(bodies: Vec<ProducedBody>) -> Produced {
        Produced { bodies, ..Default::default() }
    }

    pub fn one(body: Body, names: NameSource) -> Produced {
        Produced::bodies(vec![ProducedBody::new(body, names)])
    }

    /// An entry holding `value` and no bodies; `bytes` is what it weighs.
    pub fn payload<T: std::any::Any + Send + Sync>(value: T, bytes: usize) -> Produced {
        Produced { payload: Some(Arc::new(value)), payload_bytes: bytes, ..Default::default() }
    }

    /// The bytes this entry holds, estimated: exact for a mesh, a per-face
    /// allowance for a B-rep (a control net is a few kilobytes), the
    /// producer's own estimate for a payload.
    pub fn bytes(&self) -> usize {
        let mut n = 64 + self.payload_bytes;
        for b in &self.bodies {
            n += body_bytes(&b.body);
            if let Some(m) = &b.mesh {
                n += mesh_body_bytes(m);
            }
        }
        if let Some(m) = &self.mesh {
            n += mesh_body_bytes(&m.mesh);
        }
        for note in &self.notes {
            n += 48 + note.message.len();
        }
        n
    }
}

/// An estimate of what a body holds, for the memo's bound.
pub fn body_bytes(b: &Body) -> usize {
    match b {
        Body::Brep(s) => brep_bytes(s),
        Body::Mesh(m) => mesh_body_bytes(m),
    }
}

fn brep_bytes(s: &KSolid) -> usize {
    s.face_iter().count() * 4096 + s.edge_iter().count() * 512 + s.vertex_iter().count() * 64
}

pub fn mesh_body_bytes((mesh, topo): &MeshBody) -> usize {
    let mut n = (mesh.positions.len() + mesh.normals.len() + mesh.indices.len() + mesh.face_ids.len()) * 4 + mesh.colors.len();
    for f in &topo.faces {
        n += 256 + f.name.len() + f.edges.iter().map(|e| e.len() + 24).sum::<usize>() + f.edge_ids.len() * 4;
        n += f.aliases.iter().map(|a| a.len() + 24).sum::<usize>();
    }
    for e in &topo.edges {
        n += 160 + e.name.len() + e.polyline.len() * 24 + e.faces.iter().map(|f| f.len() + 24).sum::<usize>();
    }
    for v in &topo.vertices {
        n += 96 + v.name.len() + v.faces.iter().map(|f| f.len() + 24).sum::<usize>();
    }
    n
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// What a memo has done so far, for `cadpole serve`'s status and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    pub entries: usize,
    pub bytes: usize,
    pub limit: usize,
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
}

struct Slot {
    value: Arc<Produced>,
    bytes: usize,
    tick: u64,
}

struct Inner {
    entries: HashMap<Box<[u8]>, Slot>,
    /// Least recently used first: the tick an entry was last touched at,
    /// to its key. Ticks are unique, so this is the eviction order.
    lru: BTreeMap<u64, Box<[u8]>>,
    tick: u64,
    bytes: usize,
    version: u32,
    hits: u64,
    misses: u64,
    stores: u64,
    evictions: u64,
}

/// The bounded store. Shared by `Arc` between evaluations and threads; the
/// lock is held only for the lookup or the insert, never while the kernel
/// runs.
pub struct Memo {
    limit: usize,
    inner: Mutex<Inner>,
}

impl Default for Memo {
    fn default() -> Self {
        Memo::new()
    }
}

impl Memo {
    pub fn new() -> Memo {
        Memo::with_limit(DEFAULT_LIMIT_BYTES)
    }

    /// A memo holding at most `limit` bytes of entries (by [`Produced::bytes`]).
    pub fn with_limit(limit: usize) -> Memo {
        Memo {
            limit,
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                lru: BTreeMap::new(),
                tick: 0,
                bytes: 0,
                version: KERNEL_VERSION,
                hits: 0,
                misses: 0,
                stores: 0,
                evictions: 0,
            }),
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock means a panic while holding it; the map is still
        // consistent (nothing here panics between the two structures'
        // updates), so carry on rather than turn one panic into many.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Called at the start of every evaluation that uses this memo: a store
    /// made under another kernel version is emptied, so nothing computed by
    /// another kernel is ever served.
    pub fn attach(&self) {
        let mut inner = self.lock();
        if inner.version != KERNEL_VERSION {
            inner.entries.clear();
            inner.lru.clear();
            inner.bytes = 0;
            inner.version = KERNEL_VERSION;
        }
    }

    /// The entry for `key`, decided by comparing the key bytes in full, and
    /// touched as most recently used. Unmemoisable keys never hit.
    pub fn get(&self, key: &OpKey) -> Option<Arc<Produced>> {
        if !key.keyed {
            return None;
        }
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let (value, old) = match inner.entries.get_mut(key.bytes()) {
            Some(slot) => {
                let old = slot.tick;
                slot.tick = tick;
                (slot.value.clone(), old)
            }
            None => {
                inner.misses += 1;
                return None;
            }
        };
        inner.hits += 1;
        let k = inner.lru.remove(&old).expect("every entry is in the LRU order");
        inner.lru.insert(tick, k);
        Some(value)
    }

    /// Whether `key` has an entry, without counting it or touching the LRU
    /// order: the drag preview's question.
    pub fn contains(&self, key: &OpKey) -> bool {
        key.keyed && self.lock().entries.contains_key(key.bytes())
    }

    /// Store what `key`'s op produced, evicting least-recently-used entries
    /// until it fits. A value larger than the whole bound is not stored at
    /// all (the op keeps running from scratch), and never evicts anything
    /// to make room. Returns the shared value either way.
    pub fn put(&self, key: &OpKey, value: Produced) -> Arc<Produced> {
        let bytes = value.bytes();
        let value = Arc::new(value);
        if !key.keyed || bytes > self.limit {
            return value;
        }
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        if let Some(old) = inner.entries.remove(key.bytes()) {
            inner.lru.remove(&old.tick);
            inner.bytes -= old.bytes;
        }
        while inner.bytes + bytes > self.limit {
            let Some((&t, _)) = inner.lru.iter().next() else { break };
            let k = inner.lru.remove(&t).expect("just found");
            if let Some(slot) = inner.entries.remove(&k) {
                inner.bytes -= slot.bytes;
            }
            inner.evictions += 1;
        }
        let k: Box<[u8]> = key.bytes().into();
        inner.lru.insert(tick, k.clone());
        inner.entries.insert(k, Slot { value: value.clone(), bytes, tick });
        inner.bytes += bytes;
        inner.stores += 1;
        value
    }

    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
        inner.lru.clear();
        inner.bytes = 0;
    }

    pub fn stats(&self) -> MemoStats {
        let inner = self.lock();
        MemoStats {
            entries: inner.entries.len(),
            bytes: inner.bytes,
            limit: self.limit,
            hits: inner.hits,
            misses: inner.misses,
            stores: inner.stores,
            evictions: inner.evictions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        let hex = |d: [u8; 32]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // Longer than one block, with the length crossing the padding boundary.
        let million = vec![b'a'; 1_000_000];
        assert_eq!(hex(sha256(&million)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn keys_are_bytes_and_one_ulp_is_a_different_key() {
        let key = |d: f64| {
            let mut k = KeyBuilder::new("extrude");
            k.f64(d);
            k.finish()
        };
        assert_eq!(key(1.0), key(1.0));
        assert_ne!(key(1.0), key(f64::from_bits(1.0f64.to_bits() + 1)));
        assert_ne!(key(0.0), key(-0.0), "-0 and 0 are different bit patterns, so different keys");
    }

    #[test]
    fn an_unkeyed_operand_makes_the_op_unmemoisable() {
        let mut k = KeyBuilder::new("union");
        k.body(&BodyKey::Keyed([1; 32])).body(&BodyKey::Unkeyed);
        let key = k.finish();
        assert!(!key.is_memoisable());
        assert_eq!(key.body_key(0), BodyKey::Unkeyed);
        let memo = Memo::new();
        assert!(memo.get(&key).is_none());
        memo.put(&key, Produced::default());
        assert_eq!(memo.stats().entries, 0, "never stored");
    }

    #[test]
    fn output_keys_are_per_index_and_chain() {
        let mut k = KeyBuilder::new("split");
        k.f64(2.0);
        let key = k.finish();
        assert_ne!(key.body_key(0), key.body_key(1));
        let mut downstream = KeyBuilder::new("union");
        downstream.body(&key.body_key(0));
        let mut other = KeyBuilder::new("union");
        other.body(&key.body_key(1));
        assert_ne!(downstream.finish(), other.finish());
    }

    #[test]
    fn the_store_hits_on_equal_bytes_and_evicts_least_recently_used() {
        let memo = Memo::with_limit(64 * 3 + 10);
        let key = |i: u64| {
            let mut k = KeyBuilder::new("op");
            k.u64(i);
            k.finish()
        };
        // Every empty Produced is 64 bytes; three fit, a fourth evicts one.
        memo.put(&key(1), Produced::default());
        memo.put(&key(2), Produced::default());
        memo.put(&key(3), Produced::default());
        assert!(memo.get(&key(1)).is_some(), "touching 1 makes 2 the oldest");
        memo.put(&key(4), Produced::default());
        let s = memo.stats();
        assert_eq!(s.entries, 3);
        assert_eq!(s.evictions, 1);
        assert!(s.bytes <= s.limit);
        assert!(memo.get(&key(2)).is_none(), "2 was the least recently used");
        assert!(memo.get(&key(1)).is_some());
        assert!(memo.get(&key(3)).is_some());
        assert!(memo.get(&key(4)).is_some());
        memo.clear();
        assert_eq!(memo.stats().entries, 0);
        assert_eq!(memo.stats().bytes, 0);
    }

    #[test]
    fn a_value_over_the_whole_bound_is_not_stored_and_evicts_nothing() {
        let memo = Memo::with_limit(100);
        let small = {
            let k = KeyBuilder::new("small");
            k.finish()
        };
        memo.put(&small, Produced::default());
        let big = {
            let k = KeyBuilder::new("big");
            k.finish()
        };
        let mut p = Produced::default();
        p.notes.push(Note {
            severity: Severity::Info,
            message: "x".repeat(200),
            feature: FeatureId("f".into()),
            on_feature: true,
        });
        memo.put(&big, p);
        let s = memo.stats();
        assert_eq!(s.entries, 1);
        assert_eq!(s.evictions, 0);
        assert!(memo.get(&small).is_some());
        assert!(memo.get(&big).is_none());
    }

    #[test]
    fn canonical_forms_distinguish_types_and_arities() {
        let mut a = KeyBuilder::new("x");
        a.put(&Vec3::new(1.0, 2.0, 3.0));
        let mut b = KeyBuilder::new("x");
        b.f64(1.0).f64(2.0).f64(3.0);
        assert_ne!(a.finish(), b.finish(), "a Vec3 is not three numbers");
        let mut c = KeyBuilder::new("x");
        c.put(&vec![1.0f64, 2.0]);
        let mut d = KeyBuilder::new("x");
        d.put(&vec![1.0f64, 2.0, 3.0]);
        assert_ne!(c.finish(), d.finish());
        let mut e = KeyBuilder::new("x");
        e.str("ab").str("c");
        let mut f = KeyBuilder::new("x");
        f.str("a").str("bc");
        assert_ne!(e.finish(), f.finish(), "strings are length-prefixed");
    }
}
