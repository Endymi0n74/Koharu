#[macro_use]
pub mod utils;

pub use utils::{
    get_num_interop_threads, get_num_threads, manual_seed, set_num_interop_threads,
    set_num_threads, QEngine,
};

pub(crate) mod device;
pub(crate) mod image;
pub mod jit;
pub mod kind;
pub(crate) mod layout;
pub(crate) mod optimizer;
pub(crate) mod scalar;
pub(crate) mod stream;
pub(crate) mod tensor;
// The generated tensor wrappers stay byte-identical to the upstream tch-rs
// codegen, whose `allow(clippy::all)` silences only clippy: its `std::f64::NAN`
// defaults trip rustc's `deprecated` under CI's `-D warnings`. Exempting the
// module here keeps the generated files untouched for the next upstream sync.
#[allow(deprecated)]
pub(crate) mod tensor_fallible_generated;
pub(crate) mod tensor_generated;
