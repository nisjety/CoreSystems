# D2-A-billing-crawl progress

Unit: decision 3.11 BILLING-IDEMPOTENCY, crawl and batch only. Branch quarryv2-next, local commits only.

State: committed (ce7e4af73). Hand-over in wf-D2-A-billing-crawl.md.

Files (mine): crates/quarry-edge/src/handoff.rs (replayed flag, billable_job_id, unit test), src/routes.rs (HandoffAck.replayed, crawl and batch meters), tests/billing_handoff.rs (new), docs/BILLING.md, .plan-work/quarry-w1w2/DEPTH-FINDINGS-W2.md (D2-033 row).

Tests run through qtest.sh: billing_handoff 5 passed; lib handoff 10 passed; crawl_limits 5; job_admission 5; limits_clamp 8; drain 8; zdr_claim_floor 25; batch_handoff_check 3; org_capacity 7; quarry-runtime lib usage 13.
