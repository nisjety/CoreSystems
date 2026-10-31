# unwrap-audit.awk: lists unwrap(), expect(), panic!, unreachable!, todo! and
# unimplemented! calls that sit OUTSIDE #[cfg(test)] items (WS5 scrape-path audit).
#
# Usage (from the Quarry-v2 root):
#   awk -f scripts/unwrap-audit.awk crates/quarry-edge/src/routes.rs crates/quarry-runtime/src/pipeline.rs
# Prints FILE:LINE: TEXT for each production-code hit, and nothing when there is none.
#
# A #[cfg(test)] attribute marks the next item (a fn, a mod, a statement or a
# static); that item is skipped by counting braces, so production code after a
# test-only helper is still checked. A line ending in ';' with no brace ends a
# test-only `use` or `const`. This is a line-based audit, not a parser: read a
# hit, and read a clean file's test boundaries when a result matters.
BEGIN { skip = 0; pending = 0; depth = 0 }
{
  line = $0
  if (!skip && line ~ /#\[cfg\(test\)\]/) { pending = 1; next }
  if (pending && !skip) {
    if (line ~ /\{/) { skip = 1; pending = 0; depth = 0 }
    else if (line ~ /;[ \t]*$/) { pending = 0; next }
    else { next }
  }
  if (skip) {
    tmp = line
    o = gsub(/\{/, "{", tmp)
    tmp = line
    c = gsub(/\}/, "}", tmp)
    depth += o - c
    if (depth <= 0) { skip = 0 }
    next
  }
  if (line ~ /unwrap\(\)|\.expect\(|panic!|unreachable!|todo!|unimplemented!/) {
    print FILENAME ":" FNR ": " line
  }
}
