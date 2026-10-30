//! The models smart-commit recommends, and installing them.
//!
//! Each model is downloaded from Hugging Face, pinned to a revision and
//! checked against the SHA-256 of the file at that revision: other
//! publishers' models from their own repositories (never re-hosted), and the
//! project's fine-tunes from [`OWN_ACCOUNT`]. Any other GGUF file can be used
//! from disk instead (`model.gguf`), unverified.
//!
//! The OpenVINO devices (`npu`, `intel-gpu`, `openvino-cpu`) need the model
//! exported for OpenVINO, which only models with a pinned export have.

mod install;
mod local;
mod registry;
#[cfg(test)]
mod tests;

pub use install::*;
pub use local::*;
pub use registry::*;
