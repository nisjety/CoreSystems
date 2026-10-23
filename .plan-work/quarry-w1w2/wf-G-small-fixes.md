# G-small-fixes hand-over
Done. B-T2 committed d711a1bb (pipeline.rs helper only), docs 77f69775 (PROGRESS row + plan ledger rows). B-T3 was already committed as e7031439 by an earlier agent; nothing left to commit there.
Evidence: quarry-runtime scrape_challenge_tests 9 passed, looped 20x 0 failures; full lib looped 20x 0 failures, 739 passed (one container, default threads). Standalone repro (B-T2/repro, host, 9 threads, 60 runs): no fix 49 failures, FIX=1 0 failures.
model-gateway lib: native host run 1741 passed, 0 failed, 4 ignored (includes the other session's WS5.G grpc.rs/quarry.rs working-tree diff).
Traps: mp-test.sh in the container fails in mp-contracts build script (protoc/proto path under the Model Plane mount); the host run with WinGet protoc works. mp-test.sh also needs `cd rust &&` in the command. The repro exe glob matches a .pdb sibling: use `ls ...exe | head -1 | tr -d '*'`.
