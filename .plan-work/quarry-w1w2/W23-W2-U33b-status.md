# W2-U33b status

Built: yes (recovered by the lead session on 2026-11-02; the B4 builder left the code uncommitted).

Scope: org web policy gates on audio_url (POST /v1/audio stt), source create, schedule create (executable schedules gate every batch URL; change-monitor schedules gate target_ref), and the crawl-completion source registrar (operator deny list, human principal).

Tests (qtest-lane d, redis on): tests/url_gate_resources.rs 11 passed; quarry-edge --lib 872 passed.

Flag: none (policy leak fix).

Assumed decisions: audio_url is a web fetch under the org web policy; sources and schedules are refused at create time when denied and checked again at fetch time; the registrar has no token, so it re-checks under the baseline deny list only.

Model Plane asks: none.

Not verified: no live run; the security review of B4 ran before the code was committed (see W23-depth-lane-d.md, commit 9dc9a6868).
