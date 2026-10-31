# B-T3 design: CRLF-tolerant zdr_precheck source scan

Problem: zdr_precheck::the_zdr_stream_forwards_its_route_instead_of_dropping_it reads sse.rs with
include_str! and locates the end of `zdr_direct_stream` with tail.find("\n}\n"). The repo stores LF
in the index (i/lf) but this Windows working copy has CRLF (w/crlf), so "\n}\n" never occurs:
the function end is "\r\n}\r\n" and the expect("the function ends") panics. On Linux/CI (LF) it passes.

Fix: normalise the scanned text before searching, `include_str!("sse.rs").replace("\r\n", "\n")`.
The function becomes an owned String; the later slices borrow from it. Nothing else in the test
depends on line endings (find of the fn signature and split on `direct_infer(` / `)` are EOL-agnostic).
Only the test module of zdr_precheck.rs changes; no production code, no sse.rs change.

Why not other options: matching "\n}" alone would be fragile (nested closing braces at col 0 do not
exist in fn bodies, but "\n}" also matches a `}` of an inner raw string). Normalising is exactly what
round1-leftovers suggested and is the smallest change.

Verification: cargo test -p model-gateway --lib zdr_precheck natively (or docker runner) on the CRLF
copy; control: temporarily revert to see the old code fail (the failure was already observed in round 1,
a fresh control run is optional). Compare whole lib suite count (1730 passed, 1 failed before).
zdr_precheck.rs has no foreign hunks (git status clean). File is CRLF in the working copy, so the edit
uses lib-edit.js to preserve EOL.
