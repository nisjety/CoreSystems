# INTEGRATE (2026-10-18)
Done: tree check (only WS5.G in-flight files and .plan-work/ uncommitted, left alone), full test pass (a)-(j), one commit eb92d4ee (unneeded mut). Results and traps are in quarryv2-next-status-2026-10-17.md, last section.
Left: commit WS5.G (model-gateway grpc.rs, quarry.rs and the gateway hunks in knowledge/quarry.rs, search.rs, knowledge-client.ts, errors.ts) once that session finishes; the CRLF-sensitive test zdr_precheck::the_zdr_stream_forwards_its_route_instead_of_dropping_it could scan with .replace("\r\n","\n") (not changed here: file untouched by this branch).
Scratch runners: scratchpad/INTEGRATE/mp-test.sh, pg-test-env.sh.
