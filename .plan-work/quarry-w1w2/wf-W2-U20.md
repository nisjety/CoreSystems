# Hand-over notes, unit W2-U20 (WS14.S2)

What was done: the crawl scope contract (quarry-core), the link filter and host rule (quarry-runtime), and the optional `crawl` on `POST /v1/internal/run_page` with a report (quarry-edge), plus the openapi and RUNBOOKS sections. Inert: no caller sends `crawl` until WS14.S3.

Left: test phase (run, not written), the no-crawl golden, the robots peek (WS14.S4), the pattern differential, deployment evidence, and WS14.S3 (the first caller).

Traps found:
- `git hash-object` and `git add` apply the clean filter (core.autocrlf=true). A file with some CRLF lines and some LF lines comes out all LF. Store raw bytes with `git hash-object --no-filters` when building a blob by hand.
- HEAD moves while several units commit. Build a commit from HEAD under the git lock (`with-lock.sh git`), and check HEAD before committing. A commit built from an old copy silently drops other units' sections (this happened to RUNBOOKS; fixed by 1e2362019, history not rewritten).
- Helper scripts: a wrong positional argument to `bash -c` can make the temporary index file overwrite a message file. Check argument positions with an echo before a real commit.
- Shell: `grep -c $'\r'` does not work in this tool (it counts lines with the letter r). Use `tr -cd '\r' | wc -c`.
- `openapi.yaml` and `RUNBOOKS.md` are edited by several units in the same working copy. Insert by schema or section header, never by line number.
- The qtest redirect: `qtest.sh ... > file &` ran nothing; use `run_in_background` on the bare command.
- The edge test (`tests/crawl_scope_robots.rs`) relies on the standard fixture index; its link and count assertions are unverified.
