# Depth findings, Model Plane W2/W3 lane (branch mp-w2w3)

Append-only. Prefix each bullet with the lane and slice id.

- lead U61: `session_core` on model-plane-postgres-1 still holds a pre-0004 `browser_grants` table (kind check session/run/sensitive_approval, no policy columns). The live broker uses `browser_broker` on ingestion-postgres, which is at 0004. The stray table looks like a leftover; it was not touched. Owner: decide whether to drop it.
