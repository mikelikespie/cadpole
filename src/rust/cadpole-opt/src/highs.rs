//! One HiGHS instance, owned: created, configured, given a model, run, read
//! and destroyed. This is the only module that calls into [`crate::ffi`].
//!
//! [`Highs`] is deliberately not `Send` (the raw pointer makes it so). HiGHS
//! keeps its task scheduler per thread -- `HighsTaskExecutor`'s handle is
//! `thread_local` in 1.15 -- and an instance never needs to move: Cadpole
//! builds, runs and drops one on the thread that asked for the solve.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::ptr::NonNull;

use cadpole_core::opt::{Conflict, Model, Sense, Side};

use crate::ffi;

/// A HiGHS status or argument error, with what was being done.
pub type Result<T> = std::result::Result<T, String>;

/// What `Highs_getSolution` hands back, sized to the instance's current
/// columns and rows (which include any rows added after the model).
#[derive(Clone, Debug, Default)]
pub struct RawSolution {
    pub col_value: Vec<f64>,
    pub col_dual: Vec<f64>,
    pub row_value: Vec<f64>,
    pub row_dual: Vec<f64>,
}

pub struct Highs {
    ptr: NonNull<c_void>,
}

impl Drop for Highs {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from Highs_create and is destroyed exactly once.
        unsafe { ffi::Highs_destroy(self.ptr.as_ptr()) }
    }
}

fn cstr(s: &str) -> CString {
    CString::new(s).expect("option and info names have no NUL")
}

/// Every count HiGHS takes is a C `int`.
fn int(n: usize, what: &str) -> Result<ffi::HighsInt> {
    ffi::HighsInt::try_from(n).map_err(|_| format!("{what}: {n} is more than HiGHS can address"))
}

impl Highs {
    /// A fresh instance with HiGHS's own defaults. [`crate::configure`] is
    /// what turns it into Cadpole's solver.
    pub fn new() -> Highs {
        // SAFETY: no preconditions; HiGHS allocates and returns an instance.
        let ptr = unsafe { ffi::Highs_create() };
        Highs { ptr: NonNull::new(ptr).expect("Highs_create returned null") }
    }

    fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The library's version, e.g. "1.15.1".
    pub fn version() -> String {
        // SAFETY: HiGHS returns a pointer to a static NUL-terminated string.
        unsafe { CStr::from_ptr(ffi::Highs_version()) }.to_string_lossy().into_owned()
    }

    /// HiGHS's infinity, which Cadpole assumes is IEEE infinity.
    pub fn infinity(&self) -> f64 {
        // SAFETY: a valid instance.
        unsafe { ffi::Highs_getInfinity(self.raw()) }
    }

    fn check(status: ffi::HighsInt, what: impl FnOnce() -> String) -> Result<()> {
        if status == ffi::STATUS_ERROR { Err(what()) } else { Ok(()) }
    }

    pub fn set_bool(&mut self, option: &str, value: bool) -> Result<()> {
        let name = cstr(option);
        // SAFETY: a valid instance and a NUL-terminated name.
        let s = unsafe { ffi::Highs_setBoolOptionValue(self.raw(), name.as_ptr(), value as ffi::HighsInt) };
        Self::check(s, || format!("HiGHS refused option {option}={value}"))
    }

    pub fn set_int(&mut self, option: &str, value: i32) -> Result<()> {
        let name = cstr(option);
        // SAFETY: as above.
        let s = unsafe { ffi::Highs_setIntOptionValue(self.raw(), name.as_ptr(), value) };
        Self::check(s, || format!("HiGHS refused option {option}={value}"))
    }

    pub fn set_double(&mut self, option: &str, value: f64) -> Result<()> {
        let name = cstr(option);
        // SAFETY: as above.
        let s = unsafe { ffi::Highs_setDoubleOptionValue(self.raw(), name.as_ptr(), value) };
        Self::check(s, || format!("HiGHS refused option {option}={value}"))
    }

    pub fn set_string(&mut self, option: &str, value: &str) -> Result<()> {
        let (name, v) = (cstr(option), cstr(value));
        // SAFETY: as above.
        let s = unsafe { ffi::Highs_setStringOptionValue(self.raw(), name.as_ptr(), v.as_ptr()) };
        Self::check(s, || format!("HiGHS refused option {option}={value:?}"))
    }

    /// Hand HiGHS the model, row-wise. Infinite bounds go across as IEEE
    /// infinities, which is HiGHS's own infinity.
    pub fn pass_model(&mut self, model: &Model) -> Result<()> {
        let num_col = int(model.columns.len(), "variables")?;
        let num_row = int(model.rows.len(), "constraints")?;
        let num_nz = int(model.index.len(), "nonzeros")?;
        let cost: Vec<f64> = model.columns.iter().map(|c| c.cost).collect();
        let col_lower: Vec<f64> = model.columns.iter().map(|c| c.lower).collect();
        let col_upper: Vec<f64> = model.columns.iter().map(|c| c.upper).collect();
        let row_lower: Vec<f64> = model.rows.iter().map(|r| r.lower).collect();
        let row_upper: Vec<f64> = model.rows.iter().map(|r| r.upper).collect();
        // One start per row (the C API takes no trailing end), as C ints.
        let start: Vec<ffi::HighsInt> = model.start[..model.rows.len()].iter().map(|&s| s as ffi::HighsInt).collect();
        let index: Vec<ffi::HighsInt> = model.index.iter().map(|&j| j as ffi::HighsInt).collect();
        let sense = match model.sense {
            Sense::Minimize => ffi::OBJ_MINIMIZE,
            Sense::Maximize => ffi::OBJ_MAXIMIZE,
        };
        // HiGHS reads no pointer it has no entries for, but an empty Vec's
        // pointer is dangling rather than null; both are fine for a count of 0.
        // SAFETY: every array has the length the counts say, and outlives the
        // call; HiGHS copies the model.
        let s = unsafe {
            if model.is_mip() {
                let integrality: Vec<ffi::HighsInt> =
                    model.columns.iter().map(|c| if c.integer { ffi::VAR_INTEGER } else { ffi::VAR_CONTINUOUS }).collect();
                ffi::Highs_passMip(
                    self.raw(),
                    num_col,
                    num_row,
                    num_nz,
                    ffi::MATRIX_ROWWISE,
                    sense,
                    model.offset,
                    cost.as_ptr(),
                    col_lower.as_ptr(),
                    col_upper.as_ptr(),
                    row_lower.as_ptr(),
                    row_upper.as_ptr(),
                    start.as_ptr(),
                    index.as_ptr(),
                    model.value.as_ptr(),
                    integrality.as_ptr(),
                )
            } else {
                ffi::Highs_passLp(
                    self.raw(),
                    num_col,
                    num_row,
                    num_nz,
                    ffi::MATRIX_ROWWISE,
                    sense,
                    model.offset,
                    cost.as_ptr(),
                    col_lower.as_ptr(),
                    col_upper.as_ptr(),
                    row_lower.as_ptr(),
                    row_upper.as_ptr(),
                    start.as_ptr(),
                    index.as_ptr(),
                    model.value.as_ptr(),
                )
            }
        };
        Self::check(s, || "HiGHS refused the model".into())
    }

    /// Append one row, `lower <= sum(value * x[index]) <= upper`.
    pub fn add_row(&mut self, lower: f64, upper: f64, terms: &[(usize, f64)]) -> Result<()> {
        let index: Vec<ffi::HighsInt> = terms.iter().map(|&(j, _)| j as ffi::HighsInt).collect();
        let value: Vec<f64> = terms.iter().map(|&(_, a)| a).collect();
        let n = int(terms.len(), "row terms")?;
        // SAFETY: both arrays have `n` entries and outlive the call.
        let s = unsafe { ffi::Highs_addRow(self.raw(), lower, upper, n, index.as_ptr(), value.as_ptr()) };
        Self::check(s, || "HiGHS refused a row".into())
    }

    /// A starting point for the next run: values for some columns, which
    /// HiGHS completes (an LP for the continuous rest, a short search when
    /// integers are missing) and starts its search from.
    pub fn set_start(&mut self, values: &[(usize, f64)]) -> Result<()> {
        let index: Vec<ffi::HighsInt> = values.iter().map(|&(j, _)| j as ffi::HighsInt).collect();
        let value: Vec<f64> = values.iter().map(|&(_, v)| v).collect();
        let n = int(values.len(), "start values")?;
        // SAFETY: both arrays have `n` entries and outlive the call.
        let s = unsafe { ffi::Highs_setSparseSolution(self.raw(), n, index.as_ptr(), value.as_ptr()) };
        Self::check(s, || "HiGHS refused the starting point (a value outside its variable's bounds?)".into())
    }

    /// Solve. `Err` only when HiGHS could not even start; how the solve
    /// ended is [`Highs::model_status`].
    pub fn run(&mut self) -> Result<()> {
        // SAFETY: a valid instance with a model.
        let s = unsafe { ffi::Highs_run(self.raw()) };
        if s == ffi::STATUS_ERROR && self.model_status() == ffi::MODEL_NOTSET {
            return Err("HiGHS did not run".into());
        }
        Ok(())
    }

    /// One of the `ffi::MODEL_*` codes.
    pub fn model_status(&self) -> ffi::HighsInt {
        // SAFETY: a valid instance.
        unsafe { ffi::Highs_getModelStatus(self.raw()) }
    }

    fn int_info(&self, name: &str) -> Option<ffi::HighsInt> {
        let n = cstr(name);
        let mut v: ffi::HighsInt = 0;
        // SAFETY: a valid instance, a NUL-terminated name, a valid out pointer.
        let s = unsafe { ffi::Highs_getIntInfoValue(self.raw(), n.as_ptr(), &mut v) };
        (s != ffi::STATUS_ERROR).then_some(v)
    }

    fn double_info(&self, name: &str) -> Option<f64> {
        let n = cstr(name);
        let mut v = 0.0;
        // SAFETY: as above.
        let s = unsafe { ffi::Highs_getDoubleInfoValue(self.raw(), n.as_ptr(), &mut v) };
        (s != ffi::STATUS_ERROR).then_some(v)
    }

    fn int64_info(&self, name: &str) -> Option<i64> {
        let n = cstr(name);
        let mut v: i64 = 0;
        // SAFETY: as above.
        let s = unsafe { ffi::Highs_getInt64InfoValue(self.raw(), n.as_ptr(), &mut v) };
        (s != ffi::STATUS_ERROR).then_some(v)
    }

    /// Whether the solution arrays hold a feasible point.
    pub fn has_primal_solution(&self) -> bool {
        self.int_info("primal_solution_status") == Some(ffi::SOLUTION_FEASIBLE)
    }

    /// Whether the dual arrays hold a feasible dual (an LP solved to
    /// optimality; never a MILP).
    pub fn has_dual_solution(&self) -> bool {
        self.int_info("dual_solution_status") == Some(ffi::SOLUTION_FEASIBLE)
    }

    /// The objective HiGHS reports, before any rounding Cadpole does.
    pub fn objective(&self) -> f64 {
        self.double_info("objective_function_value").unwrap_or(f64::NAN)
    }

    /// The MILP's relative gap, a fraction (HiGHS's log prints it as a
    /// percentage); infinite before the first point is found.
    pub fn mip_gap(&self) -> f64 {
        self.double_info("mip_gap").unwrap_or(f64::INFINITY)
    }

    /// The MILP's best bound on the objective, in the problem's own sense:
    /// no point is better. `None` before the search has one.
    pub fn mip_bound(&self) -> Option<f64> {
        self.double_info("mip_dual_bound").filter(|b| b.is_finite())
    }

    pub fn mip_nodes(&self) -> u64 {
        self.int64_info("mip_node_count").unwrap_or(0).max(0) as u64
    }

    pub fn simplex_iterations(&self) -> u64 {
        self.int_info("simplex_iteration_count").unwrap_or(0).max(0) as u64
    }

    pub fn num_cols(&self) -> usize {
        // SAFETY: a valid instance.
        unsafe { ffi::Highs_getNumCol(self.raw()) }.max(0) as usize
    }

    pub fn num_rows(&self) -> usize {
        // SAFETY: a valid instance.
        unsafe { ffi::Highs_getNumRow(self.raw()) }.max(0) as usize
    }

    /// Values, row activities, reduced costs and row duals, as HiGHS left
    /// them.
    pub fn solution(&self) -> Result<RawSolution> {
        let (n, m) = (self.num_cols(), self.num_rows());
        let mut s = RawSolution { col_value: vec![0.0; n], col_dual: vec![0.0; n], row_value: vec![0.0; m], row_dual: vec![0.0; m] };
        // SAFETY: each array has the instance's current column or row count.
        let st = unsafe {
            ffi::Highs_getSolution(
                self.raw(),
                s.col_value.as_mut_ptr(),
                s.col_dual.as_mut_ptr(),
                s.row_value.as_mut_ptr(),
                s.row_dual.as_mut_ptr(),
            )
        };
        Self::check(st, || "HiGHS has no solution to read".into())?;
        Ok(s)
    }

    /// An irreducible infeasible subsystem of the model as passed, by
    /// HiGHS's elastic filter and then its deletion filter. For a MILP it is
    /// the LP relaxation's. Only the first `model_rows` rows are reported,
    /// so rows added after the model (Cadpole's no-good cuts) never appear.
    pub fn iis(&mut self, model_rows: usize) -> Result<Conflict> {
        let (n, m) = (self.num_cols(), self.num_rows());
        let (mut num_col, mut num_row): (ffi::HighsInt, ffi::HighsInt) = (0, 0);
        let mut col_index = vec![0 as ffi::HighsInt; n.max(1)];
        let mut row_index = vec![0 as ffi::HighsInt; m.max(1)];
        let mut col_bound = vec![0 as ffi::HighsInt; n.max(1)];
        let mut row_bound = vec![0 as ffi::HighsInt; m.max(1)];
        // SAFETY: the index and bound arrays have room for every column and
        // row, which bounds what an IIS can hold; the per-column and per-row
        // status arrays are not asked for (null).
        let s = unsafe {
            ffi::Highs_getIis(
                self.raw(),
                &mut num_col,
                &mut num_row,
                col_index.as_mut_ptr(),
                row_index.as_mut_ptr(),
                col_bound.as_mut_ptr(),
                row_bound.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        Self::check(s, || "HiGHS could not compute a conflict".into())?;
        let side = |b: ffi::HighsInt| match b {
            ffi::IIS_BOUND_LOWER => Some(Side::Lower),
            ffi::IIS_BOUND_UPPER => Some(Side::Upper),
            ffi::IIS_BOUND_BOXED => Some(Side::Both),
            _ => None,
        };
        let mut conflict = Conflict::default();
        for k in 0..(num_row.max(0) as usize).min(m) {
            let r = row_index[k] as usize;
            if let Some(s) = side(row_bound[k])
                && r < model_rows
            {
                conflict.rows.push((r, s));
            }
        }
        for k in 0..(num_col.max(0) as usize).min(n) {
            if let Some(s) = side(col_bound[k]) {
                conflict.columns.push((col_index[k] as usize, s));
            }
        }
        conflict.rows.sort_by_key(|&(r, _)| r);
        conflict.columns.sort_by_key(|&(j, _)| j);
        Ok(conflict)
    }
}

impl Default for Highs {
    fn default() -> Highs {
        Highs::new()
    }
}
