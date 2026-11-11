//! The parts of HiGHS's C API that Cadpole calls, declared by hand from
//! `highs/interfaces/highs_c_api.h` at 1.15.1 (no bindgen, no new
//! crates). Nothing else in this crate touches a raw pointer; `highs.rs` is
//! the safe wrapper over these.
//!
//! `HighsInt` is a C `int`: the HConfig.h the Bazel module generates leaves
//! `HIGHSINT64` undefined. `crate::highs::tests::the_c_api_is_the_one_declared`
//! checks the version and the infinity these declarations assume.
//!
//! The calls that run a solver are `extern "C-unwind"`. HiGHS catches most C++
//! exceptions itself, but not every one (an out-of-memory `std::bad_alloc` in
//! some paths, a few internal `std::logic_error`s), and a C++ exception that
//! unwinds into Rust through a plain `extern "C"` declaration is undefined
//! behaviour. Through `C-unwind` it is defined: it runs the Rust destructors
//! above it and, since Rust cannot catch a foreign exception, ends the
//! process with a message rather than corrupting it.

use std::os::raw::{c_char, c_double, c_int, c_void};

pub type HighsInt = c_int;

pub const STATUS_ERROR: HighsInt = -1;

pub const VAR_CONTINUOUS: HighsInt = 0;
pub const VAR_INTEGER: HighsInt = 1;

pub const OBJ_MINIMIZE: HighsInt = 1;
pub const OBJ_MAXIMIZE: HighsInt = -1;

pub const MATRIX_ROWWISE: HighsInt = 2;

/// `kHighsSolutionStatusFeasible`: the solution arrays hold a feasible point.
pub const SOLUTION_FEASIBLE: HighsInt = 2;

// kHighsModelStatus*
pub const MODEL_NOTSET: HighsInt = 0;
pub const MODEL_LOAD_ERROR: HighsInt = 1;
pub const MODEL_MODEL_ERROR: HighsInt = 2;
pub const MODEL_PRESOLVE_ERROR: HighsInt = 3;
pub const MODEL_SOLVE_ERROR: HighsInt = 4;
pub const MODEL_POSTSOLVE_ERROR: HighsInt = 5;
pub const MODEL_EMPTY: HighsInt = 6;
pub const MODEL_OPTIMAL: HighsInt = 7;
pub const MODEL_INFEASIBLE: HighsInt = 8;
pub const MODEL_UNBOUNDED_OR_INFEASIBLE: HighsInt = 9;
pub const MODEL_UNBOUNDED: HighsInt = 10;
pub const MODEL_OBJECTIVE_BOUND: HighsInt = 11;
pub const MODEL_OBJECTIVE_TARGET: HighsInt = 12;
pub const MODEL_TIME_LIMIT: HighsInt = 13;
pub const MODEL_ITERATION_LIMIT: HighsInt = 14;
pub const MODEL_UNKNOWN: HighsInt = 15;
pub const MODEL_SOLUTION_LIMIT: HighsInt = 16;
pub const MODEL_INTERRUPT: HighsInt = 17;

// kIisStrategy* (lp_data/HConst.h). The option's own help text numbers these
// differently; the enum is what the code tests.
pub const IIS_FROM_LP: HighsInt = 2;
pub const IIS_IRREDUCIBLE: HighsInt = 4;

// kIisBoundStatus* (lp_data/HighsIis.h): which bounds of a row or column
// take part in the conflict.
pub const IIS_BOUND_LOWER: HighsInt = 2;
pub const IIS_BOUND_UPPER: HighsInt = 3;
pub const IIS_BOUND_BOXED: HighsInt = 4;

unsafe extern "C" {
    pub fn Highs_create() -> *mut c_void;
    pub fn Highs_destroy(highs: *mut c_void);
    pub fn Highs_version() -> *const c_char;

    pub fn Highs_setBoolOptionValue(highs: *mut c_void, option: *const c_char, value: HighsInt) -> HighsInt;
    pub fn Highs_setIntOptionValue(highs: *mut c_void, option: *const c_char, value: HighsInt) -> HighsInt;
    pub fn Highs_setDoubleOptionValue(highs: *mut c_void, option: *const c_char, value: c_double) -> HighsInt;
    pub fn Highs_setStringOptionValue(highs: *mut c_void, option: *const c_char, value: *const c_char) -> HighsInt;

    pub fn Highs_getModelStatus(highs: *const c_void) -> HighsInt;
    pub fn Highs_getIntInfoValue(highs: *const c_void, info: *const c_char, value: *mut HighsInt) -> HighsInt;
    pub fn Highs_getDoubleInfoValue(highs: *const c_void, info: *const c_char, value: *mut c_double) -> HighsInt;
    pub fn Highs_getInt64InfoValue(highs: *const c_void, info: *const c_char, value: *mut i64) -> HighsInt;
    pub fn Highs_getSolution(
        highs: *const c_void,
        col_value: *mut c_double,
        col_dual: *mut c_double,
        row_value: *mut c_double,
        row_dual: *mut c_double,
    ) -> HighsInt;
    pub fn Highs_getNumCol(highs: *const c_void) -> HighsInt;
    pub fn Highs_getNumRow(highs: *const c_void) -> HighsInt;
    pub fn Highs_getInfinity(highs: *const c_void) -> c_double;
}

unsafe extern "C-unwind" {
    pub fn Highs_passLp(
        highs: *mut c_void,
        num_col: HighsInt,
        num_row: HighsInt,
        num_nz: HighsInt,
        a_format: HighsInt,
        sense: HighsInt,
        offset: c_double,
        col_cost: *const c_double,
        col_lower: *const c_double,
        col_upper: *const c_double,
        row_lower: *const c_double,
        row_upper: *const c_double,
        a_start: *const HighsInt,
        a_index: *const HighsInt,
        a_value: *const c_double,
    ) -> HighsInt;
    pub fn Highs_passMip(
        highs: *mut c_void,
        num_col: HighsInt,
        num_row: HighsInt,
        num_nz: HighsInt,
        a_format: HighsInt,
        sense: HighsInt,
        offset: c_double,
        col_cost: *const c_double,
        col_lower: *const c_double,
        col_upper: *const c_double,
        row_lower: *const c_double,
        row_upper: *const c_double,
        a_start: *const HighsInt,
        a_index: *const HighsInt,
        a_value: *const c_double,
        integrality: *const HighsInt,
    ) -> HighsInt;
    pub fn Highs_run(highs: *mut c_void) -> HighsInt;
    pub fn Highs_setSparseSolution(
        highs: *mut c_void,
        num_entries: HighsInt,
        index: *const HighsInt,
        value: *const c_double,
    ) -> HighsInt;
    pub fn Highs_addRow(
        highs: *mut c_void,
        lower: c_double,
        upper: c_double,
        num_new_nz: HighsInt,
        index: *const HighsInt,
        value: *const c_double,
    ) -> HighsInt;
    pub fn Highs_getIis(
        highs: *mut c_void,
        iis_num_col: *mut HighsInt,
        iis_num_row: *mut HighsInt,
        col_index: *mut HighsInt,
        row_index: *mut HighsInt,
        col_bound: *mut HighsInt,
        row_bound: *mut HighsInt,
        col_status: *mut HighsInt,
        row_status: *mut HighsInt,
    ) -> HighsInt;
}
