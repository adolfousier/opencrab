//! Which project a working directory belongs to.
//!
//! Sessions get an auto-generated title on their first turn but were never
//! linked to a project at that moment. The only path that created the link was
//! the `/cd` handler, so a session carried a correct `working_directory` and a
//! `project_id` of NULL unless someone happened to change directory by hand,
//! and every per-project view and cost rollup under-reported by however many
//! sessions that was (#1445).
//!
//! The rule itself is the one `/cd` has always used: slugify the directory's
//! basename and compare it against each project's slugified name. It lives
//! here rather than inside the `/cd` handler so the two call sites cannot
//! answer the same question differently.
//!
//! Matching on a basename is knowingly imprecise. Two unrelated directories
//! sharing a basename collapse onto one project, and a checkout living under
//! an unrelated directory name is unreachable. That is a property of the
//! schema: `projects` has no path and no remote to compare against, only a
//! name. Fixing it means a migration, which is deliberately not this change.

use crate::db::models::Project;
use crate::services::file::slugify_project_name;

/// The basename of `working_directory`, slugified for comparison.
///
/// A trailing separator is ignored, so `~/src/thing/` and `~/src/thing` are
/// the same directory, which is how a user types it about half the time.
fn directory_slug(working_directory: &str) -> Option<String> {
    let trimmed = working_directory.trim_end_matches(['/', '\\']);
    let name = std::path::Path::new(trimmed).file_name()?.to_str()?;
    let slug = slugify_project_name(name);
    (!slug.is_empty()).then_some(slug)
}

/// The project this working directory names, if any.
///
/// Ties go to the first project in the list. Two projects slugging to the same
/// name is already ambiguous at creation time; picking arbitrarily here is no
/// worse than leaving the session unlinked, and the caller logs what it chose.
pub fn match_by_directory<'a>(
    working_directory: &str,
    projects: &'a [Project],
) -> Option<&'a Project> {
    let dir = directory_slug(working_directory)?;
    projects
        .iter()
        .find(|p| slugify_project_name(&p.name) == dir)
}
