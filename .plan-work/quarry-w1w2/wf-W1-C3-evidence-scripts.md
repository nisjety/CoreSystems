# W1-C3-evidence-scripts: hand-over
Built (not run): WS4.B guard + flag, WS4.C scratch NATS script + gated JetStream test, WS4.A probes and clippy triage, WS4.D/E image probes, WS4.G token probe, runbook and RUNBOOKS text.
Expected results: C3-evidence-scripts-progress.md. Left: every run (see depth file). Traps: a lost backslash when a heredoc carries `\n` into a node script (use String.fromCharCode(92)); `${VAR:?text}` text must not contain an apostrophe (bash -n fails); Git Bash path rewriting for docker absolute paths needs MSYS_NO_PATHCONV=1.
