//! Conservative temporary-space budgeting for a native conversion.
//!
//! The conversion never runs "and sees how far it gets": a copy that fills the
//! user's disk halfway through would leave debris beside the database the
//! application still depends on. The budget below is deliberately pessimistic
//! and is stated in bytes, because the alternative - assuming DuckDB's measured
//! compression ratio - would make the estimate depend on how compressible this
//! particular user's history happens to be.
//!
//! Measured ratios from an earlier conversion may raise the estimate, never
//! lower it (see [`ConversionSpaceObservation`]).
//!
//! # What this budget deliberately does not cover
//!
//! The SQLite source is kept until a later verified startup retires it, and
//! whether it is then renamed in place or copied aside is an open maintainer
//! question. A rename is atomic and free; a copy is safer against a downgraded
//! older build but needs another whole source's worth of space. The budget
//! below assumes the rename, so if the copy is chosen this formula has to gain
//! a fourth term.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use super::NativeDatabaseError;

/// Floor for the working space a conversion needs beyond the two database
/// files: DuckDB's spill directory plus the staging tables reconciliation
/// writes its changed rows through.
const MINIMUM_WORKSPACE_BYTES: u64 = 64 * 1024 * 1024;

/// What an earlier conversion of the same source actually produced.
///
/// Only used to raise the estimate. A candidate that came out smaller than its
/// source says nothing about the next source's compressibility, so the ratio is
/// clamped at 1.0 rather than credited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionSpaceObservation {
  pub source_bytes: u64,
  pub candidate_bytes: u64,
  pub finalized_bytes: u64,
}

/// The byte budget for one conversion, and the space the workspace volume
/// currently reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversionSpacePlan {
  pub source_database_bytes: u64,
  /// The `-wal` and `-shm` sidecars, which hold committed rows the snapshot
  /// must read.
  pub source_journal_bytes: u64,
  pub source_total_bytes: u64,
  pub candidate_estimate_bytes: u64,
  pub finalized_estimate_bytes: u64,
  pub workspace_estimate_bytes: u64,
  pub required_bytes: u64,
  pub available_bytes: u64,
}

/// The three components of the budget, without touching the filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversionSpaceRequirement {
  pub candidate_estimate_bytes: u64,
  pub finalized_estimate_bytes: u64,
  pub workspace_estimate_bytes: u64,
  pub required_bytes: u64,
}

/// `required = candidate + finalized + workspace`.
///
/// All three terms coexist: reconciliation captures a second candidate while
/// the finalized file it is updating is still on disk, and the staging tables
/// that carry the changed rows live in the workspace until the transaction
/// commits. Each database term starts at the full source size - no compression
/// is assumed - and a measured ratio above 1.0 raises it.
pub fn conversion_space_requirement(
  source_total_bytes: u64,
  measured: Option<ConversionSpaceObservation>,
) -> ConversionSpaceRequirement {
  let candidate_estimate_bytes = scale(
    source_total_bytes,
    measured.map(|observed| (observed.candidate_bytes, observed.source_bytes)),
  );
  let finalized_estimate_bytes = scale(
    source_total_bytes,
    measured.map(|observed| (observed.finalized_bytes, observed.source_bytes)),
  );
  let workspace_estimate_bytes = source_total_bytes.max(MINIMUM_WORKSPACE_BYTES);
  ConversionSpaceRequirement {
    candidate_estimate_bytes,
    finalized_estimate_bytes,
    workspace_estimate_bytes,
    required_bytes: candidate_estimate_bytes
      .saturating_add(finalized_estimate_bytes)
      .saturating_add(workspace_estimate_bytes),
  }
}

/// `source_total * produced / consumed`, rounded up, and never below
/// `source_total`.
fn scale(source_total_bytes: u64, ratio: Option<(u64, u64)>) -> u64 {
  let Some((produced, consumed)) = ratio.filter(|(_, consumed)| *consumed > 0) else {
    return source_total_bytes;
  };
  let scaled = (u128::from(source_total_bytes) * u128::from(produced))
    .div_ceil(u128::from(consumed));
  u64::try_from(scaled)
    .unwrap_or(u64::MAX)
    .max(source_total_bytes)
}

/// Measure the source, budget the conversion, and refuse with numbers when the
/// workspace volume cannot hold it.
pub fn plan_conversion_space(
  source_database: &Path,
  workspace: &Path,
  measured: Option<ConversionSpaceObservation>,
) -> Result<ConversionSpacePlan, NativeDatabaseError> {
  let source_database_bytes =
    file_bytes(source_database).ok_or_else(|| NativeDatabaseError::Unavailable {
      path: source_database.to_owned(),
    })?;
  let source_journal_bytes = ["-wal", "-shm"]
    .into_iter()
    .filter_map(|suffix| {
      let mut sidecar = source_database.as_os_str().to_os_string();
      sidecar.push(suffix);
      file_bytes(&PathBuf::from(sidecar))
    })
    .fold(0_u64, u64::saturating_add);
  let source_total_bytes = source_database_bytes.saturating_add(source_journal_bytes);
  let requirement = conversion_space_requirement(source_total_bytes, measured);
  let available_bytes = available_bytes(workspace)?;

  let plan = ConversionSpacePlan {
    source_database_bytes,
    source_journal_bytes,
    source_total_bytes,
    candidate_estimate_bytes: requirement.candidate_estimate_bytes,
    finalized_estimate_bytes: requirement.finalized_estimate_bytes,
    workspace_estimate_bytes: requirement.workspace_estimate_bytes,
    required_bytes: requirement.required_bytes,
    available_bytes,
  };
  if available_bytes < plan.required_bytes {
    return Err(NativeDatabaseError::InsufficientWorkspace {
      path: workspace.to_owned(),
      required_bytes: plan.required_bytes,
      available_bytes,
    });
  }
  Ok(plan)
}

fn file_bytes(path: &Path) -> Option<u64> {
  std::fs::metadata(path)
    .ok()
    .filter(std::fs::Metadata::is_file)
    .map(|metadata| metadata.len())
}

/// Free bytes on the volume holding `workspace`, taken from the mounted volume
/// whose mount point is the longest prefix of it.
///
/// `sysinfo` is already a Core dependency for hardware collection, so this adds
/// no new library for one number.
fn available_bytes(workspace: &Path) -> Result<u64, NativeDatabaseError> {
  let canonical = workspace
    .canonicalize()
    .unwrap_or_else(|_| workspace.to_owned());
  let resolved = strip_verbatim_prefix(&canonical);
  let disks = sysinfo::Disks::new_with_refreshed_list();
  disks
    .iter()
    .filter(|disk| resolved.starts_with(strip_verbatim_prefix(disk.mount_point())))
    .max_by_key(|disk| disk.mount_point().as_os_str().len())
    .map(sysinfo::Disk::available_space)
    .ok_or_else(|| NativeDatabaseError::WorkspaceSpaceUnknown {
      path: workspace.to_owned(),
    })
}

/// Put a Windows extended-length path back into the ordinary form the mount
/// table uses.
///
/// `Path::canonicalize` returns verbatim paths on Windows - `\\?\C:\Users\...`,
/// or `\\?\UNC\server\share\...` for a network path - while `sysinfo` reports
/// mount points as `C:\` and `\\server\share`. `Path::starts_with` compares
/// whole components including the prefix, so the two never match and the
/// preflight would report the free space as unknowable on a supported
/// platform. The rewrite is textual on purpose: the prefixes are only produced
/// by Windows, but the function has to be compiled and testable everywhere.
fn strip_verbatim_prefix(path: &Path) -> Cow<'_, Path> {
  let Some(text) = path.to_str() else {
    return Cow::Borrowed(path);
  };
  if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
    return Cow::Owned(PathBuf::from(format!(r"\\{share}")));
  }
  // A verbatim device path (`\\?\Volume{...}`) has no ordinary spelling, so
  // it is left alone rather than turned into something that is not a path.
  if let Some(rest) = text.strip_prefix(r"\\?\")
    && rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    && rest.as_bytes().get(1) == Some(&b':')
  {
    return Cow::Owned(PathBuf::from(rest));
  }
  Cow::Borrowed(path)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn budgets_two_databases_and_a_workspace_without_assuming_compression() {
    let source = 4 * 1024 * 1024 * 1024_u64;
    let requirement = conversion_space_requirement(source, None);
    assert_eq!(requirement.candidate_estimate_bytes, source);
    assert_eq!(requirement.finalized_estimate_bytes, source);
    assert_eq!(requirement.workspace_estimate_bytes, source);
    assert_eq!(requirement.required_bytes, source * 3);
  }

  #[test]
  fn a_measured_conversion_that_shrank_does_not_lower_the_budget() {
    let source = 22 * 1024 * 1024_u64;
    let measured = ConversionSpaceObservation {
      source_bytes: 100,
      candidate_bytes: 40,
      finalized_bytes: 25,
    };
    let requirement = conversion_space_requirement(source, Some(measured));
    assert_eq!(requirement.candidate_estimate_bytes, source);
    assert_eq!(requirement.finalized_estimate_bytes, source);
  }

  #[test]
  fn a_measured_conversion_that_grew_raises_the_budget() {
    let source = 1_000_u64;
    let measured = ConversionSpaceObservation {
      source_bytes: 100,
      candidate_bytes: 150,
      finalized_bytes: 301,
    };
    let requirement = conversion_space_requirement(source, Some(measured));
    assert_eq!(requirement.candidate_estimate_bytes, 1_500);
    // Rounded up: a budget must never be short by a partial byte.
    assert_eq!(requirement.finalized_estimate_bytes, 3_010);
    assert_eq!(
      requirement.required_bytes,
      1_500 + 3_010 + MINIMUM_WORKSPACE_BYTES
    );
  }

  #[test]
  fn a_small_source_still_reserves_the_workspace_floor() {
    let requirement = conversion_space_requirement(1_024, None);
    assert_eq!(
      requirement.workspace_estimate_bytes,
      MINIMUM_WORKSPACE_BYTES
    );
    assert_eq!(requirement.required_bytes, 2_048 + MINIMUM_WORKSPACE_BYTES);
  }

  /// Pure path arithmetic, so it runs on every platform even though only
  /// Windows produces the input.
  #[test]
  fn a_windows_extended_length_path_is_matched_against_an_ordinary_mount_point() {
    let disk = Path::new(r"\\?\C:\Users\someone\AppData\Roaming\hv");
    assert_eq!(
      strip_verbatim_prefix(disk).as_ref(),
      Path::new(r"C:\Users\someone\AppData\Roaming\hv")
    );

    let network = Path::new(r"\\?\UNC\server\share\hv");
    assert_eq!(
      strip_verbatim_prefix(network).as_ref(),
      Path::new(r"\\server\share\hv")
    );

    // A volume GUID path has no drive-letter spelling, and an ordinary path is
    // never rewritten.
    let volume = Path::new(r"\\?\Volume{9f3a}\hv");
    assert_eq!(strip_verbatim_prefix(volume).as_ref(), volume);
    for ordinary in [r"C:\Users\someone", "/Users/someone", "/"] {
      let ordinary = Path::new(ordinary);
      assert_eq!(strip_verbatim_prefix(ordinary).as_ref(), ordinary);
    }

    // The comparison the rewrite exists for. Only Windows parses a drive or
    // UNC prefix into components, so only there can the match be asserted;
    // everywhere else a backslash is an ordinary character and both paths are
    // one component.
    #[cfg(windows)]
    {
      assert!(
        strip_verbatim_prefix(disk).starts_with(strip_verbatim_prefix(Path::new(r"C:\")))
      );
      assert!(
        strip_verbatim_prefix(network)
          .starts_with(strip_verbatim_prefix(Path::new(r"\\server\share")))
      );
      // Without the rewrite the verbatim prefix is its own component, so the
      // mount point never matches and the free space reads as unknowable.
      assert!(!disk.starts_with(Path::new(r"C:\")));
    }
  }

  #[test]
  fn an_absent_source_is_refused_before_any_work() {
    let directory = tempfile::tempdir().unwrap();
    let error =
      plan_conversion_space(&directory.path().join("absent.db"), directory.path(), None)
        .unwrap_err();
    assert!(matches!(error, NativeDatabaseError::Unavailable { .. }));
  }

  #[test]
  fn the_plan_counts_the_write_ahead_log_beside_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("hv-database.db");
    std::fs::write(&source, vec![0_u8; 4_096]).unwrap();
    std::fs::write(
      directory.path().join("hv-database.db-wal"),
      vec![0_u8; 2_048],
    )
    .unwrap();

    let plan = plan_conversion_space(&source, directory.path(), None).unwrap();
    assert_eq!(plan.source_database_bytes, 4_096);
    assert_eq!(plan.source_journal_bytes, 2_048);
    assert_eq!(plan.source_total_bytes, 6_144);
    assert_eq!(
      plan.required_bytes,
      conversion_space_requirement(6_144, None).required_bytes
    );
    assert!(plan.available_bytes > 0);
  }
}
