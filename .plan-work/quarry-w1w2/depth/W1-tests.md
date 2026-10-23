# W1 wave test phase: failures and timeouts

- [severity low] [kind test] scripts/tests/principal-registry-parity.test.sh (timeout: killed by the 1000 s wrapper, exit 124; 21 ok and 0 not ok before the kill; the shadowing-launcher check and the three bootstrap runs were not reached) | scripts/tests/principal-registry-parity.test.sh at the repo root (last changed in bea28d6c, 2026-10-08, so pre-existing) | wave test phase: run once, not retried; the script header says a few minutes on Windows, this run did not finish within 1000 s
