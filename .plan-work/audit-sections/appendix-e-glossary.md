## Appendix E. Glossary

### Verevon terms

- **ADR (Architecture Decision Record).** A short record of one architecture decision and its trade-offs. "ADR needed" means leadership must decide before the affected items are built.
- **Plane.** A service group in the CoresSystem monorepo that owns one domain. Control Plane: identity, users, organisations, billing, sessions, audit, quotas, entitlements. Data Plane v2: documents, chunks, embeddings, retrieval, graph, wiki. Ingestion Plane: captures evidence and persists durable knowledge through Data Plane contracts (it holds Quarry v2 and integration-corev2). Model Plane: reasoning, sessions and runs, inference, execution loops, capabilities, sandboxes, browser grants, cost. Application Plane: collaborative and realtime workspace projections and notifications. Frontend Plane: the Verevon v3 user interface and the same-origin gateway. Support Plane: the RustDesk-compatible remote-support rendezvous and relay. Channel Plane: future, documentation only. Planes do not read each other's databases.
- **Space.** Verevon's governed room shared by people and agents.
- **Quarry (Quarry v2).** Verevon's web acquisition engine in the Ingestion Plane: crawl, scrape, extract and browser automation. The Model Plane proposes browser actions; Quarry executes or rejects them.
- **BrowserBroker.** The Model Plane service that issues browser grants, which are the permission for one run to act in a browser.
- **integration-corev2 (IC).** The Ingestion Plane service that executes provider actions and connector calls.
- **ZDR (Zero Data Retention).** A setting under which content must not be persisted or logged. In Verevon it has to hold in every plane that could store content.
- **W1 to W5.** The implementation waves in section 9.
- **OD n, D n, H n, E n, I n, S n.m.** Identifiers of open decisions, decisions and plan rows in Verevon's own planning documents (for example "open decision OD19"). They are kept as written so the team can look them up.

### General terms

- **ACL.** Access control list: the rules saying who may do what to a resource.
- **API / SDK / CLI.** Application programming interface; software development kit; command-line interface.
- **AX (accessibility tree).** The structured description of a page that screen readers use; browser agents read it to find elements.
- **BFF.** Backend for frontend: a server layer that serves one frontend and calls the other services on its behalf.
- **BYO.** Bring your own (for example your own keys or models).
- **C2PA.** A standard for attaching provenance information to generated media.
- **CAPTCHA.** A challenge meant to tell humans from bots.
- **CAS.** Content-addressed storage: files are stored and found by a hash of their content.
- **CDP.** Chrome DevTools Protocol: the remote-control interface of Chromium browsers.
- **CEL.** Common Expression Language: a small language for writing policy conditions.
- **CI.** Continuous integration (automated build and test runs).
- **CSP.** Content Security Policy: a browser header that limits what a page may load or run.
- **DLP.** Data loss prevention.
- **DM.** Direct message.
- **DND.** Do not disturb.
- **DOM.** Document Object Model: the live tree of a web page.
- **DOCX / OOXML.** Microsoft Office's zipped XML document formats.
- **GDPR / HIPAA.** European and United States data-protection laws.
- **HITL.** Human in the loop: a person approves or steers an agent step.
- **HMAC.** A keyed hash used to sign data so tampering is detectable.
- **IDE.** Integrated development environment (a code editor).
- **IPC.** Inter-process communication.
- **JSONL.** JSON Lines: one JSON object per line.
- **JWT.** JSON Web Token: a signed token that carries claims.
- **LLM.** Large language model.
- **MCP.** Model Context Protocol: an open protocol for giving agents tools and data.
- **MSIX.** The Windows app packaging format used by Teams, Slack, Claude, Codex and Manus on this audit's machine.
- **NATS.** A messaging system used between Verevon services.
- **NIP.** Nostr Implementation Possibility: a numbered specification of the Nostr protocol (used by Buzz).
- **OCR.** Optical character recognition (reading text from images).
- **OTLP / OTel.** OpenTelemetry protocol and project, a standard for traces, metrics and logs.
- **OTP.** One-time password (the code sent by email or an authenticator app).
- **PII.** Personally identifiable information.
- **PTY.** A pseudo-terminal, used to run an interactive shell for an agent.
- **PWA.** Progressive web app (a website that installs and works like an app).
- **REA.** The reverse-engineering toolkit used in this audit to analyse the installed apps. In provenance tags, [REA] means a fact observed through REA on a shipped app.
- **RPC.** Remote procedure call.
- **S3.** Amazon's object storage API, also offered by compatible stores such as MinIO.
- **SCIM.** A standard for provisioning users and groups from an identity provider.
- **SEA.** Node.js single executable application: a Node program packaged as one binary.
- **SFU.** Selective forwarding unit: a media server that relays audio and video in group calls.
- **SIEM.** Security information and event management (a central security log and alerting system).
- **SPA.** Single-page application.
- **SRI.** Subresource integrity: a hash that lets a browser verify a loaded script.
- **SSE.** Server-sent events: a one-way stream from server to browser over HTTP.
- **SSO.** Single sign-on.
- **SSRF.** Server-side request forgery: tricking a server into fetching an internal address.
- **STT / ASR / TTS.** Speech to text; automatic speech recognition; text to speech.
- **TLS.** Transport Layer Security (encryption of network connections).
- **TTL.** Time to live: how long something is kept or valid.
- **UI / UX.** User interface; user experience.
- **VM / microVM.** Virtual machine; a very small, fast-starting virtual machine used for sandboxing.
- **WASM.** WebAssembly, a portable binary format that runs in browsers and sandboxes.
- **WS.** WebSocket, a two-way connection between browser and server.

