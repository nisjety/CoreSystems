# Licensing boundary

**This is technical research for engineering decisions, not legal advice.** Confirm the conclusions below with counsel before this ships to production, especially anything touching Section 13 obligations or the `hbb_common` license gap noted below.

## What's actually licensed, and how

| Repository | SPDX | Evidence |
|---|---|---|
| `rustdesk/rustdesk` (client) | **AGPL-3.0** | `LICENCE` file (British spelling); GitHub API `license.spdx_id: "AGPL-3.0"` |
| `rustdesk/rustdesk-server` (hbbs/hbbr) | **AGPL-3.0** | `LICENSE` file; GitHub API confirms — same license family as the client, just a filename spelling difference |
| `rustdesk/hbb_common` (protocol crate) | **No license file, no `license` field in `Cargo.toml`, GitHub API reports `license: null`** | See "The hbb_common gap" below |

RustDesk Inc. also sells **"RustDesk Server Pro"** — a paid, closed-source management layer on top of the open server (device groups, address book, SSO, custom clients), marketed via `rustdesk.com/pricing`. This reads as a separate proprietary product layered on the open-core server, not classic dual-licensing of the identical AGPL codebase — but the actual Pro EULA text could not be fetched during this research (403/429 on the specific license page) and was not otherwise reviewed. Not relevant to the plan below, which uses only the free/OSS server.

### The `hbb_common` gap

The repository that actually defines the wire protocol Verevon depends on — the `.proto` schemas, the crypto handshake, the password-hashing scheme — carries **no license grant at all** in its own repository, separate from the AGPL-3.0 grant on `rustdesk/rustdesk` proper (which pulls it in as a git submodule). This is worth having counsel weigh in on explicitly: an unlicensed repository doesn't default to "no restrictions" under copyright law, but the plan below (read for protocol facts, reimplement independently) is deliberately structured to not depend on `hbb_common` carrying any particular license, for the reasons in the next section.

## Three scenarios, and which one this project is in

These are treated very differently under AGPL/GPL-style copyleft, though exactly where the lines fall is fact-specific:

**(a) Running the official server as a separate, unmodified process that our code talks to over the network.** The FSF's own GPL FAQ treats sockets/pipes as "communication mechanisms normally used between separate programs," which keeps each side a separate work rather than a combined one. Verevon's Support Plane runs unmodified `hbbs`/`hbbr` binaries; `remote-core` is a client of that network protocol, conceptually no different from a browser talking to a web server. **This is the lowest-friction scenario, and it's the one Support Plane's deployment is in.**

**(b) Copying or adapting RustDesk source into our own repository.** This is the scenario the top-level spec explicitly forbids ("Do not copy AGPL source into Verevon code unless explicitly justified"), and for good reason — it's the one with the clearest copyleft attachment, including AGPL Section 13's network-use obligation for anything run as a service (see below). **Nothing in `remote-core` does this**: no `.proto` file, no generated code, no Rust source, no Flutter/Dart source from any RustDesk repository is present in this codebase.

**(c) Reading RustDesk source purely to extract protocol facts (message names, field numbers, field types, sequence of operations) and reimplementing those independently.** Copyright protects a particular *expression* of an idea, not the underlying protocol, format, or interface — the fact that "message X has fields A, B, C sent in this order" is not itself the kind of thing copyright locks up, even though the specific *code* implementing it is. This is the traditional basis for clean-room interoperability work. **This is the scenario `remote-core`'s protocol layer is in**: `docs/rustdesk-protocol.md` documents the facts (with citations, for traceability), and [`src/protocol/rustdesk/`](../src/protocol/rustdesk/) is original TypeScript that reimplements those facts — hand-written protobuf wire encoding (varint/zigzag/length-delimited primitives, golden-tested against Google's own public protobuf specification example, not against anything from RustDesk), message-specific encode/decode functions using the verified field numbers, and a from-scratch password-hash implementation using the Web Crypto API.

Scenario (c) is also the most fact-sensitive of the three: the closer independently-written code gets to mirroring the original's actual code structure, comments, variable names, or arbitrary constants (as opposed to just the objective protocol facts), the more it can look like scenario (b) in substance. Concretely, in this codebase: field *numbers* and *types* are reproduced (they are the interoperability facts — get one wrong and the protocol breaks), but no `.proto` file text, no comments, and no code structure from `hbb_common` are reproduced anywhere. Message and field names in `src/protocol/rustdesk/messages/types.ts` are the protocol's actual names (also facts — a `LoginRequest` needs to be called that to be findable/maintainable against the documented protocol) but the type definitions, encode/decode logic, and control flow are independently written.

## AGPL-3.0 Section 13, in plain terms

Section 13 ("Remote Network Interaction") is what distinguishes AGPL from ordinary GPL: if an organization **modifies** AGPL-licensed software and lets other users interact with that **modified version remotely over a network**, it must offer those users the modified version's Corresponding Source, free of charge. Plain GPL has no equivalent — running a modified server privately and only exposing it over a network never triggers a release obligation under GPL; AGPL closes exactly that gap.

Practical read for Support Plane:

- **Running the official, unmodified `hbbs`/`hbbr` binaries or images does not, by itself, create a release obligation** — the source is already public upstream.
- The obligation is triggered by **modifying** the AGPL program itself (patches, custom builds, forked behavior) and then operating that modified version as a network-facing service.
- **Configuration** (env vars, CLI flags, `-k`/license-key options, port choices) is a different question from "recompiled with source changes" — exactly where that line sits for a specific deployment approach is the kind of fact-specific question to run past counsel before finalizing anything beyond plain configuration.
- If Support Plane ever needs a source patch to hbbs/hbbr (rather than pure configuration), that patch — and the fact that it's being run as a network service — is exactly the point to loop in counsel, not something to decide unilaterally in code review.

## Dependency scan (informational, not exhaustive)

A quick pass over the client's `Cargo.toml`/`vcpkg.json` found the dependency tree overwhelmingly permissive (MIT/Apache-2.0/BSD/ISC — `serde`, `tokio`, `rustls`, `sodiumoxide`, `libvpx`, `libaom`, `libyuv`, `opus`). **One notable exception, flagged for completeness even though it doesn't touch this project**: the official client vendors its own FFmpeg build (`res/vcpkg/ffmpeg/portfile.cmake`) compiled with `--enable-gpl`, which reclassifies that specific FFmpeg build from its default LGPL-2.1+ to full **GPL** — a stronger copyleft than everything else in the tree, used for the client's hardware H264/H265 codec path on at least Windows. `remote-core` never touches this Rust codebase, this build, or any FFmpeg output, so it does not attach to anything in this repository — it's recorded here only because it directly answers "is any GPL, as opposed to AGPL/MIT/Apache, code involved anywhere," and because it would matter immediately if anyone later considered vendoring or linking against the official client's native build artifacts.

## `remote-core`'s own runtime dependencies

Two, both permissively licensed and neither derived from RustDesk: `libsodium-wrappers` (ISC — the crypto primitives) and `fzstd` (MIT — a pure-JS Zstandard *decompressor*, added 2026-09-08 because RustDesk always zstd-compresses cursor bitmaps). Zstandard itself is an open format (RFC 8878); using a decoder for it creates no relationship to RustDesk's code.

## What this means in practice for `@verevon/remote-core`

- `packages/remote-core` contains **zero lines copied from any RustDesk repository**. Its protocol layer is an independent, original implementation informed by documented protocol facts (scenario c).
- `apps/Support Plane` runs the **official, unmodified** `rustdesk-server` (hbbs/hbbr) as a separate network service (scenario a) — see `apps/Support Plane/README.md` for the deployment shape.
- If Support Plane's deployment ever needs to *patch* hbbs/hbbr source (not just configure it), that crosses into a different scenario and should be flagged to counsel before shipping, per Section 13 above.
- `remote-core`'s own code is Verevon's, under whatever license Verevon chooses for its proprietary codebase (this monorepo currently carries no root `LICENSE` file, consistent with the rest of CoreSystem's private packages) — it is not AGPL-encumbered by virtue of talking to an AGPL server over a network, per scenario (a).
