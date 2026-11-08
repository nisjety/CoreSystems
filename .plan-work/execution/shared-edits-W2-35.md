## WS19-S3
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: in GeneratedFiles (about line 3238), inside the For card after the download anchor, render a chip when file.check?.level is 'passed' (i18n.tr('Kontrollert', 'Checked')), 'warnings' (i18n.tr('Advarsler', 'Warnings')) or 'failed' (i18n.tr('Feil funnet', 'Errors found')). 'skipped' or no check shows no chip. Each chip's title must say the check only reads the file structure and that Office is not confirmed to open it, e.g. passed: i18n.tr('Filstrukturen er kontrollert. Det betyr ikke at Office åpner filen.', 'The file structure was checked. This does not mean Office opens the file.'). Then add chip tests to ChatMessages.generated-files.test.tsx, and a rule for the chip class in apps/Frontend Plane/verevonv3/src/styles/global.css near .verevon-chat-generated-files (about line 26202) if no existing badge class fits.

## WS21-S5a
shared-edit scripts/tests/zdr-contract-sync.test.sh: the GOLDENS pin at line 107 for apps/Model Plane/api/contracts/zdr-posture-matrix.json changes from a6ff6879c7ee55e17688ee6eaf81ca2119cfa05721cc476719b3a3c09c8f4e1a to 9f61e99040d6895248fc8e22cec2adaf60e14dba7fefa1f3bf8ad7ccf149c201. The pin at line 114 for zdr-refusals.json changes from 066c5876e676b0b796922944e824e86f4c63070f0d830c9da86f33ff018d0249 to 025c8c19838b969d4c8ac667d15b6b3d7893a2d368f534602b6acadc0a9ad724. Both new digests were re-verified with sha256sum this round.

## WS21-S5a
shared-edit, Ingestion and Frontend owners, byte-identical re-vendor: copy apps/Model Plane/api/contracts/zdr-refusals.json and zdr-refusals.json.sha256 over apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/zdr-refusals/ (the copy there is still 066c5876). Create apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json and .sha256, which are absent today.

## WS21-S5a
shared-edit apps/Model Plane/go/services/capability-core/internal/api/availability.go: add "cap.browser.observe": {} to genericGlobalHealthCapabilityIDs (lines 114-127). The browser-owner lane admits only cap.browser.open, so the generic list is the right place. The file's comment calls widening this list a decision, so record it. This clears the one red execution-core lib test, and without it every live attestation is refused with 403.

## WS21-S5a
shared-edit apps/Model Plane/rust/crates/mp-content-safety/src/trust.rs TOOL_TRUST: add ("browser.open", BrowserScraped) and ("browser.observe", BrowserScraped), plus the parity-test case in that crate.

