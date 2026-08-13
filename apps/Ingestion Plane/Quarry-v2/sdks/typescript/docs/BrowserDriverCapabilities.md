# BrowserDriverCapabilities

Explicit browser-driver routing claims. A missing or `false` value is never
inferred from a provider name. Agent runs require both `isolatedEgress` and
`securityEvidence`, regardless of optional requested capabilities.

## Properties

Name | Type | Meaning
------------ | ------------- | -------------
`persistentProfile` | boolean | Durable profile support for an approved run.
`devtoolsTrace` | boolean | Redacted CDP trace support.
`downloadsToArtifacts` | boolean | Browser downloads can be quarantined and admitted as Quarry artifacts.
`uploadsFromArtifacts` | boolean | Only approved Quarry artifacts can be attached to native file inputs.
`fullVisualFidelity` | boolean | Full Chromium-compatible visual surface.
`isolatedEgress` | boolean | Browser traffic uses Quarry's governed egress boundary.
`securityEvidence` | boolean | The claimed browser request surface has current security proof.
`atomicTargetActions` | boolean | Snapshot target verification and effect are atomic in the driver.

No capability is promised until it is present and `true` in the selected
driver's response.
