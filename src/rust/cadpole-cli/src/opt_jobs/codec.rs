//! The solution cache's file format: opt models, the options that
//! decide an answer, and solutions, written so that every number comes back
//! bit for bit.
//!
//! **Why not protobuf.** Protos are Cadpole's persistence format, but a cache
//! entry is not a document: it is derived, disposable and read by nothing
//! outside this module, so a schema in `proto/cadpole/v1` would buy generated
//! Swift and TypeScript nobody reads and a second place for the encoding to
//! drift. JSON is out for a better reason: serde_json's float parsing is not
//! exact without its `float_roundtrip` feature, and an answer that comes back
//! one ulp off is not the answer. So: little-endian fixed-width fields, `f64`s
//! as their bit patterns, `Option`s with a tag byte, lengths checked against
//! what is left before anything is allocated.
//!
//! **A file** is an 8-byte magic, the format number, what kind of body it
//! holds, the body's length and its SHA-256, then the body. A file whose
//! checksum, kind or length is wrong is refused whole -- the cache treats it
//! as a miss and deletes it -- so a torn write after a power cut, a file from
//! a future format or a stray file in the directory can never be read as an
//! answer.

use cadpole_engine::cadpole_core::memo::sha256;
use cadpole_engine::cadpole_core::opt::{Column, Conflict, Model, Options, Row, Sense, Side, Solution, Status};

/// The first eight bytes of every file this module writes.
pub const MAGIC: &[u8; 8] = b"cadp-opt";

/// Bumped when the encoding of any body changes. A file of another format is
/// refused, so an old cache is simply a cold one.
pub const FORMAT: u32 = 1;

/// What a file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A cache entry: one question's answer.
    Answer = 1,
    /// A job's input: the question and the job's limits.
    Problem = 2,
    /// A job's outcome, whatever it was.
    Outcome = 3,
}

/// Where an answer was computed.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// In an evaluation.
    Inline,
    /// By the job with this id.
    Job(String),
}

/// One cache entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// The cache key it is filed under ([`super::cache::Cache::key`]).
    pub key: [u8; 32],
    /// The solver that answered, as `Solver::identity` says.
    pub identity: String,
    /// `os/arch` of the machine that answered.
    pub platform: String,
    /// When it was filed, in milliseconds since the Unix epoch.
    pub made_unix_ms: u64,
    pub source: Source,
    /// `Some(limit)` for a job's time-limited answer that its solve accepted
    /// by asking for `time_limit=limit`: kept, and said to be.
    pub limited: Option<f64>,
    /// How long the solve took, in milliseconds.
    pub elapsed_ms: f64,
    pub solutions: Vec<Solution>,
}

/// A job's input: everything the child needs, and nothing it must trust.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// The cache key the answer is filed under.
    pub key: [u8; 32],
    /// `cadpole_core::opt::digest` of the model, the options and the parent's
    /// solver identity. The child recomputes it and refuses a mismatch.
    pub digest: [u8; 32],
    /// The parent's solver identity: a child whose solver differs refuses
    /// the job rather than file an answer under another solver's key.
    pub identity: String,
    pub model: Model,
    /// The question's options: `deadline` empty, nothing about where it runs.
    pub options: Options,
    /// The job's own time limit, in seconds; `None` for none.
    pub time_limit: Option<f64>,
    /// Whether the solve asked for that limit (`time_limit=`), so a limited
    /// answer may be kept.
    pub accept_limited: bool,
    /// The child's address-space limit, in MB; 0 for none.
    pub mem_mb: u64,
    /// The problem's feature id and the line of its `solve()`.
    pub origin: String,
    /// Which process asked: "cadpole serve (pid 812)".
    pub owner: String,
}

/// What a job produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub key: [u8; 32],
    pub elapsed_ms: f64,
    pub solutions: Vec<Solution>,
    /// Anything the job has to say about how it ended.
    pub message: String,
}

// ------------------------------------------------------------------ framing

const HEADER: usize = 8 + 4 + 4 + 8 + 32;

/// A whole file: header and body.
pub fn frame(kind: Kind, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&(kind as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&sha256(body));
    out.extend_from_slice(body);
    out
}

/// The body of a file of `kind`, or why the file is not one.
pub fn unframe(kind: Kind, file: &[u8]) -> Result<&[u8], String> {
    if file.len() < HEADER {
        return Err(format!("{} bytes is shorter than a header", file.len()));
    }
    if &file[..8] != MAGIC {
        return Err("not a cadpole opt file".into());
    }
    let format = u32::from_le_bytes(file[8..12].try_into().unwrap());
    if format != FORMAT {
        return Err(format!("format {format}, not {FORMAT}"));
    }
    let k = u32::from_le_bytes(file[12..16].try_into().unwrap());
    if k != kind as u32 {
        return Err(format!("holds kind {k}, not {}", kind as u32));
    }
    let len = u64::from_le_bytes(file[16..24].try_into().unwrap());
    let body = &file[HEADER..];
    if body.len() as u64 != len {
        return Err(format!("the body is {} bytes, the header says {len}", body.len()));
    }
    if sha256(body) != file[24..56] {
        return Err("the body does not match its checksum".into());
    }
    Ok(body)
}

// ---------------------------------------------------------------- writing

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    fn bytes32(&mut self, v: &[u8; 32]) {
        self.0.extend_from_slice(v);
    }
    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn opt_f64(&mut self, v: Option<f64>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.f64(x);
            }
            None => self.u8(0),
        }
    }
    fn opt_u64(&mut self, v: Option<u64>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.u64(x);
            }
            None => self.u8(0),
        }
    }
    fn f64s(&mut self, v: &[f64]) {
        self.u64(v.len() as u64);
        self.0.reserve(v.len() * 8);
        for &x in v {
            self.f64(x);
        }
    }
    fn opt_f64s(&mut self, v: &Option<Vec<f64>>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.f64s(x);
            }
            None => self.u8(0),
        }
    }
}

// ---------------------------------------------------------------- reading

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    fn new(b: &'a [u8]) -> R<'a> {
        R { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.b.len() - self.at < n {
            return Err(format!("truncated: wanted {n} bytes at {}, {} left", self.at, self.b.len() - self.at));
        }
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn bool(&mut self) -> Result<bool, String> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(format!("{b} is not a bool")),
        }
    }
    fn bytes32(&mut self) -> Result<[u8; 32], String> {
        Ok(self.take(32)?.try_into().unwrap())
    }
    /// A count of items of `size` bytes each, checked against what is left
    /// so a corrupt length cannot ask for a huge allocation.
    fn count(&mut self, size: usize) -> Result<usize, String> {
        let n = self.u64()?;
        let left = (self.b.len() - self.at) as u64;
        if n.saturating_mul(size as u64) > left {
            return Err(format!("a length of {n} with {left} bytes left"));
        }
        Ok(n as usize)
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.count(1)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "a string that is not UTF-8".to_owned())
    }
    fn opt_f64(&mut self) -> Result<Option<f64>, String> {
        Ok(if self.bool()? { Some(self.f64()?) } else { None })
    }
    fn opt_u64(&mut self) -> Result<Option<u64>, String> {
        Ok(if self.bool()? { Some(self.u64()?) } else { None })
    }
    fn f64s(&mut self) -> Result<Vec<f64>, String> {
        let n = self.count(8)?;
        (0..n).map(|_| self.f64()).collect()
    }
    fn opt_f64s(&mut self) -> Result<Option<Vec<f64>>, String> {
        Ok(if self.bool()? { Some(self.f64s()?) } else { None })
    }
    fn done(&self) -> Result<(), String> {
        if self.at != self.b.len() {
            return Err(format!("{} bytes left over", self.b.len() - self.at));
        }
        Ok(())
    }
}

// --------------------------------------------------------------- the parts

fn status_code(s: Status) -> u8 {
    // Every variant by name, so a new status fails to compile here
    // rather than being written as something it is not.
    match s {
        Status::Optimal => 1,
        Status::Infeasible => 2,
        Status::Unbounded => 3,
        Status::InfeasibleOrUnbounded => 4,
        Status::TimeLimit => 5,
        Status::NodeLimit => 6,
        Status::IterationLimit => 7,
        Status::Interrupted => 8,
        Status::Pending => 9,
        Status::Error => 10,
    }
}

fn status_of(c: u8) -> Result<Status, String> {
    Ok(match c {
        1 => Status::Optimal,
        2 => Status::Infeasible,
        3 => Status::Unbounded,
        4 => Status::InfeasibleOrUnbounded,
        5 => Status::TimeLimit,
        6 => Status::NodeLimit,
        7 => Status::IterationLimit,
        8 => Status::Interrupted,
        9 => Status::Pending,
        10 => Status::Error,
        other => return Err(format!("status {other}")),
    })
}

fn side_code(s: Side) -> u8 {
    match s {
        Side::Lower => 1,
        Side::Upper => 2,
        Side::Both => 3,
    }
}

fn side_of(c: u8) -> Result<Side, String> {
    Ok(match c {
        1 => Side::Lower,
        2 => Side::Upper,
        3 => Side::Both,
        other => return Err(format!("side {other}")),
    })
}

fn put_solution(w: &mut W, s: &Solution) {
    w.u8(status_code(s.status));
    w.opt_f64s(&s.values);
    w.opt_f64s(&s.activities);
    w.opt_f64(s.objective);
    w.opt_f64s(&s.row_duals);
    w.opt_f64s(&s.reduced_costs);
    w.opt_f64(s.mip_gap);
    w.opt_f64(s.bound);
    w.u64(s.nodes);
    w.u64(s.iterations);
    match &s.conflict {
        Some(c) => {
            w.u8(1);
            for part in [&c.rows, &c.columns] {
                w.u64(part.len() as u64);
                for &(i, side) in part.iter() {
                    w.u64(i as u64);
                    w.u8(side_code(side));
                }
            }
        }
        None => w.u8(0),
    }
    w.str(&s.message);
}

fn get_solution(r: &mut R<'_>) -> Result<Solution, String> {
    let mut s = Solution::without_point(status_of(r.u8()?)?);
    s.values = r.opt_f64s()?;
    s.activities = r.opt_f64s()?;
    s.objective = r.opt_f64()?;
    s.row_duals = r.opt_f64s()?;
    s.reduced_costs = r.opt_f64s()?;
    s.mip_gap = r.opt_f64()?;
    s.bound = r.opt_f64()?;
    s.nodes = r.u64()?;
    s.iterations = r.u64()?;
    if r.bool()? {
        let mut c = Conflict::default();
        for part in [&mut c.rows, &mut c.columns] {
            let n = r.count(9)?;
            for _ in 0..n {
                let i = r.u64()? as usize;
                part.push((i, side_of(r.u8()?)?));
            }
        }
        s.conflict = Some(c);
    }
    s.message = r.str()?;
    Ok(s)
}

fn put_solutions(w: &mut W, sols: &[Solution]) {
    w.u64(sols.len() as u64);
    for s in sols {
        put_solution(w, s);
    }
}

fn get_solutions(r: &mut R<'_>) -> Result<Vec<Solution>, String> {
    // A solution is at least a few dozen bytes; 16 is a safe floor for the
    // length check.
    let n = r.count(16)?;
    (0..n).map(|_| get_solution(r)).collect()
}

fn put_model(w: &mut W, m: &Model) {
    w.bool(m.sense == Sense::Maximize);
    w.f64(m.offset);
    w.u64(m.columns.len() as u64);
    for c in &m.columns {
        w.str(&c.name);
        w.f64(c.lower);
        w.f64(c.upper);
        w.bool(c.integer);
        w.f64(c.cost);
    }
    w.u64(m.rows.len() as u64);
    for r in &m.rows {
        w.str(&r.name);
        w.f64(r.lower);
        w.f64(r.upper);
    }
    w.u64(m.start.len() as u64);
    for &s in &m.start {
        w.u64(s as u64);
    }
    w.u64(m.index.len() as u64);
    for &j in &m.index {
        w.u32(j);
    }
    w.f64s(&m.value);
}

fn get_model(r: &mut R<'_>) -> Result<Model, String> {
    let sense = if r.bool()? { Sense::Maximize } else { Sense::Minimize };
    let mut m = Model::new(sense);
    m.offset = r.f64()?;
    let n = r.count(8 + 8 + 8 + 1 + 8)?;
    m.columns.reserve(n);
    for _ in 0..n {
        let name = r.str()?;
        let (lower, upper, integer, cost) = (r.f64()?, r.f64()?, r.bool()?, r.f64()?);
        m.columns.push(Column { name, lower, upper, integer, cost });
    }
    let n = r.count(8 + 8 + 8)?;
    m.rows.reserve(n);
    for _ in 0..n {
        let name = r.str()?;
        let (lower, upper) = (r.f64()?, r.f64()?);
        m.rows.push(Row { name, lower, upper });
    }
    let n = r.count(8)?;
    m.start = (0..n).map(|_| r.u64().map(|s| s as usize)).collect::<Result<_, _>>()?;
    let n = r.count(4)?;
    m.index = (0..n).map(|_| r.u32()).collect::<Result<_, _>>()?;
    m.value = r.f64s()?;
    m.validate().map_err(|e| format!("the model does not validate: {e}"))?;
    Ok(m)
}

/// The options that make up the question. Nothing about where or how long
/// the solve runs is written: those are the job's, not the question's.
fn put_options(w: &mut W, o: &Options) {
    w.opt_f64(o.time_limit);
    w.f64(o.mip_rel_gap);
    w.u64(o.count as u64);
    w.u64(o.distinct.len() as u64);
    for &j in &o.distinct {
        w.u64(j as u64);
    }
    w.opt_u64(o.max_nodes);
}

fn get_options(r: &mut R<'_>) -> Result<Options, String> {
    let time_limit = r.opt_f64()?;
    let mip_rel_gap = r.f64()?;
    let count = r.u64()? as usize;
    let n = r.count(8)?;
    let distinct = (0..n).map(|_| r.u64().map(|j| j as usize)).collect::<Result<_, _>>()?;
    let max_nodes = r.opt_u64()?;
    Ok(Options { time_limit, mip_rel_gap, count, distinct, max_nodes, ..Options::default() })
}

fn put_source(w: &mut W, s: &Source) {
    match s {
        Source::Inline => w.u8(0),
        Source::Job(id) => {
            w.u8(1);
            w.str(id);
        }
    }
}

fn get_source(r: &mut R<'_>) -> Result<Source, String> {
    Ok(match r.u8()? {
        0 => Source::Inline,
        1 => Source::Job(r.str()?),
        other => return Err(format!("source {other}")),
    })
}

// ------------------------------------------------------------- the bodies

pub fn encode_answer(a: &Answer) -> Vec<u8> {
    let mut w = W::default();
    w.bytes32(&a.key);
    w.str(&a.identity);
    w.str(&a.platform);
    w.u64(a.made_unix_ms);
    put_source(&mut w, &a.source);
    w.opt_f64(a.limited);
    w.f64(a.elapsed_ms);
    put_solutions(&mut w, &a.solutions);
    frame(Kind::Answer, &w.0)
}

pub fn decode_answer(file: &[u8]) -> Result<Answer, String> {
    let mut r = R::new(unframe(Kind::Answer, file)?);
    let a = Answer {
        key: r.bytes32()?,
        identity: r.str()?,
        platform: r.str()?,
        made_unix_ms: r.u64()?,
        source: get_source(&mut r)?,
        limited: r.opt_f64()?,
        elapsed_ms: r.f64()?,
        solutions: get_solutions(&mut r)?,
    };
    r.done()?;
    if a.solutions.is_empty() {
        return Err("an answer with no solutions".into());
    }
    Ok(a)
}

pub fn encode_problem(p: &Problem) -> Vec<u8> {
    let mut w = W::default();
    w.bytes32(&p.key);
    w.bytes32(&p.digest);
    w.str(&p.identity);
    put_model(&mut w, &p.model);
    put_options(&mut w, &p.options);
    w.opt_f64(p.time_limit);
    w.bool(p.accept_limited);
    w.u64(p.mem_mb);
    w.str(&p.origin);
    w.str(&p.owner);
    frame(Kind::Problem, &w.0)
}

pub fn decode_problem(file: &[u8]) -> Result<Problem, String> {
    let mut r = R::new(unframe(Kind::Problem, file)?);
    let p = Problem {
        key: r.bytes32()?,
        digest: r.bytes32()?,
        identity: r.str()?,
        model: get_model(&mut r)?,
        options: get_options(&mut r)?,
        time_limit: r.opt_f64()?,
        accept_limited: r.bool()?,
        mem_mb: r.u64()?,
        origin: r.str()?,
        owner: r.str()?,
    };
    r.done()?;
    Ok(p)
}

pub fn encode_outcome(o: &Outcome) -> Vec<u8> {
    let mut w = W::default();
    w.bytes32(&o.key);
    w.f64(o.elapsed_ms);
    put_solutions(&mut w, &o.solutions);
    w.str(&o.message);
    frame(Kind::Outcome, &w.0)
}

pub fn decode_outcome(file: &[u8]) -> Result<Outcome, String> {
    let mut r = R::new(unframe(Kind::Outcome, file)?);
    let o = Outcome { key: r.bytes32()?, elapsed_ms: r.f64()?, solutions: get_solutions(&mut r)?, message: r.str()? };
    r.done()?;
    Ok(o)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const INF: f64 = f64::INFINITY;

    pub(crate) fn model() -> Model {
        let mut m = Model::new(Sense::Maximize);
        m.offset = 0.25;
        m.columns.push(Column { name: "w".into(), lower: 8.0, upper: 20.0, integer: false, cost: 2.0 });
        m.columns.push(Column { name: "d ü".into(), lower: -INF, upper: INF, integer: false, cost: 1.0 });
        m.columns.push(Column { name: "pick".into(), lower: 0.0, upper: 1.0, integer: true, cost: -0.0 });
        m.push_row(Row { name: "perimeter".into(), lower: -INF, upper: 30.0 }, &[(0, 1.0), (1, 1.0)]);
        m.push_row(Row { name: "either".into(), lower: 1.0, upper: 1.0 }, &[(1, 0.1 + 0.2), (2, -3.0)]);
        m
    }

    /// Every field, including the awkward bits: -0.0, a NaN with a payload,
    /// both infinities, `None` against an empty vector.
    fn awkward() -> Solution {
        let mut s = Solution::without_point(Status::NodeLimit);
        s.values = Some(vec![20.0, -0.0, 1.0]);
        s.activities = Some(vec![]);
        s.objective = Some(f64::from_bits(0x7ff8_0000_dead_beef));
        s.row_duals = None;
        s.reduced_costs = Some(vec![INF, -INF, f64::MIN_POSITIVE]);
        s.mip_gap = Some(0.0123);
        s.bound = None;
        s.nodes = u64::MAX;
        s.iterations = 7;
        s.conflict = Some(Conflict { rows: vec![(1, Side::Both)], columns: vec![(0, Side::Lower), (2, Side::Upper)] });
        s.message = "kept from job 3f9a2c1b7e44".into();
        s
    }

    fn bits(s: &Solution) -> Vec<u64> {
        let mut out = vec![status_code(s.status) as u64, s.nodes, s.iterations];
        for v in [&s.values, &s.activities, &s.row_duals, &s.reduced_costs] {
            match v {
                Some(v) => {
                    out.push(1);
                    out.extend(v.iter().map(|x| x.to_bits()));
                }
                None => out.push(0),
            }
        }
        for v in [s.objective, s.mip_gap, s.bound] {
            out.push(v.map_or(u64::MAX - 1, f64::to_bits));
        }
        out
    }

    #[test]
    fn an_answer_comes_back_bit_for_bit() {
        let a = Answer {
            key: [7; 32],
            identity: "HiGHS 1.15.1 (threads=1, random_seed=0, cadpole-opt 1)".into(),
            platform: "linux/x86_64".into(),
            made_unix_ms: 1_793_000_000_000,
            source: Source::Job("3f9a2c1b7e44".into()),
            limited: Some(600.0),
            elapsed_ms: 31_200.5,
            solutions: vec![awkward(), Solution::without_point(Status::Optimal)],
        };
        let file = encode_answer(&a);
        let b = decode_answer(&file).unwrap();
        assert_eq!(b.key, a.key);
        assert_eq!((b.identity.as_str(), b.platform.as_str(), b.made_unix_ms), (a.identity.as_str(), a.platform.as_str(), a.made_unix_ms));
        assert_eq!((b.source.clone(), b.limited, b.elapsed_ms.to_bits()), (a.source.clone(), a.limited, a.elapsed_ms.to_bits()));
        assert_eq!(b.solutions.len(), 2);
        for (x, y) in a.solutions.iter().zip(&b.solutions) {
            assert_eq!(bits(x), bits(y));
            assert_eq!(x.conflict, y.conflict);
            assert_eq!(x.message, y.message);
        }
        // `PartialEq` would say NaN != NaN; the bits above are the claim.
        assert!(b.solutions[0].objective.unwrap().is_nan());
        assert!(b.solutions[0].values.as_ref().unwrap()[1].is_sign_negative(), "-0.0 stays -0.0");
        assert_eq!(b.solutions[0].activities, Some(vec![]), "an empty vector is not None");
    }

    #[test]
    fn a_problem_comes_back_whole_and_validated() {
        let p = Problem {
            key: [1; 32],
            digest: [2; 32],
            identity: "s".into(),
            model: model(),
            options: Options { time_limit: Some(600.0), mip_rel_gap: 0.0, count: 3, distinct: vec![2], max_nodes: Some(20), ..Options::default() },
            time_limit: None,
            accept_limited: true,
            mem_mb: 8192,
            origin: "sizes (main.star:12)".into(),
            owner: "cadpole serve (pid 812)".into(),
        };
        let q = decode_problem(&encode_problem(&p)).unwrap();
        assert_eq!(q, p);
        assert_eq!(q.model.columns[2].cost.to_bits(), (-0.0f64).to_bits());
        assert_eq!(q.model.value[2].to_bits(), (0.1f64 + 0.2).to_bits());
        let o = Outcome { key: [3; 32], elapsed_ms: 1.5, solutions: vec![awkward()], message: "done".into() };
        let back = decode_outcome(&encode_outcome(&o)).unwrap();
        assert_eq!((back.key, back.message.as_str()), (o.key, "done"));
        assert_eq!(bits(&back.solutions[0]), bits(&o.solutions[0]));
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused_whole() {
        let a = Answer {
            key: [9; 32],
            identity: "s".into(),
            platform: "p".into(),
            made_unix_ms: 1,
            source: Source::Inline,
            limited: None,
            elapsed_ms: 0.0,
            solutions: vec![Solution::without_point(Status::Infeasible)],
        };
        let good = encode_answer(&a);
        assert!(decode_answer(&good).is_ok());
        // One flipped bit anywhere in the body fails the checksum.
        for at in [HEADER, HEADER + 40, good.len() - 1] {
            let mut bad = good.clone();
            bad[at] ^= 0x10;
            assert!(decode_answer(&bad).unwrap_err().contains("checksum"), "flipped byte {at}");
        }
        // A torn write: any prefix.
        for n in [0, 7, HEADER - 1, HEADER, good.len() - 1] {
            assert!(decode_answer(&good[..n]).is_err(), "a {n}-byte prefix");
        }
        // Another kind, another format, another magic.
        assert!(decode_problem(&good).unwrap_err().contains("kind"));
        let mut future = good.clone();
        future[8] = 99;
        assert!(decode_answer(&future).unwrap_err().contains("format"));
        let mut stranger = good.clone();
        stranger[0] = b'K';
        assert!(decode_answer(&stranger).unwrap_err().contains("not a cadpole opt file"));
        // A body that checks out but claims a vector longer than the file
        // is refused before anything is allocated.
        let mut w = W::default();
        w.bytes32(&[9; 32]);
        w.str("s");
        w.str("p");
        w.u64(1);
        put_source(&mut w, &Source::Inline);
        w.opt_f64(None);
        w.f64(0.0);
        w.u64(u64::MAX);
        assert!(decode_answer(&frame(Kind::Answer, &w.0)).unwrap_err().contains("length"));
    }

    #[test]
    fn a_model_that_does_not_validate_is_refused() {
        let mut m = model();
        m.index[1] = 0; // columns no longer increasing in row 0
        let p = Problem {
            key: [0; 32],
            digest: [0; 32],
            identity: String::new(),
            model: m,
            options: Options::default(),
            time_limit: None,
            accept_limited: false,
            mem_mb: 0,
            origin: String::new(),
            owner: String::new(),
        };
        assert!(decode_problem(&encode_problem(&p)).unwrap_err().contains("validate"));
    }
}
