// Package lifecycle groups the stored contract notices of a procedure and says what changed between
// them, following the DFØ identifier rules (docs/SOURCES.md):
//
//   - the procedure-identifier is the grouping key: every notice of a procedure carries it, and a
//     change notice is a notice of the same procedure;
//   - (notice-identifier, notice-version) identifies a notice version;
//   - the Doffin identifier and the TED publication number are lookup aliases and are never used as
//     identity;
//   - planning notices carry no procedure-identifier and stand alone.
//
// A change notice is a separate notice with its own identifier and its own Doffin id, so the sync
// stores it like any other. It names the notice it changes by identifier and version, or by TED
// publication number, which a stored body cannot be matched to; the procedure is the link that does
// not depend on either. Group orders the notices of a procedure by when they were issued. Diff
// compares two of them field by field, with no model: what moved, what was added, what was removed.
//
// The package is pure. It reads parsed notices and keeps nothing.
package lifecycle
