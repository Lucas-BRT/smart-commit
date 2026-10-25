//! Rules-based detection for lockfiles/generated files.

use crate::context::RepoContext;
use crate::diff::{self, FileDiff};

/// Whether the message comes from the file list alone: every staged file
/// was written by a tool (see [`diff::machine_written`]).
pub fn by_rules(ctx: &RepoContext, files: &[FileDiff]) -> bool {
    !files.is_empty()
        && files
            .iter()
            .all(|f| diff::machine_written(f, &ctx.generated))
}
