
# BrowserObservation


## Properties

Name | Type
------------ | -------------
`runId` | string
`step` | number
`url` | string
`title` | string
`snapshot` | { [key: string]: any; }
`domSummary` | { [key: string]: any; }
`screenshotArtifactId` | string
`visualObservationArtifactId` | string
`evidenceDeltaArtifactId` | string
`consoleSummary` | Array&lt;{ [key: string]: any; }&gt;
`networkSummary` | Array&lt;{ [key: string]: any; }&gt;
`egressReceipts` | Array&lt;{ [key: string]: any; }&gt;
`dialogs` | Array&lt;{ [key: string]: any; }&gt; — observed JavaScript dialogs; none is accepted automatically.
`policyDenials` | Array&lt;string&gt;
`actionOutcome` | [ActionOutcome](ActionOutcome.md)
`observationDelta` | [ObservationDelta](ObservationDelta.md)
`challenge` | { [key: string]: any; }
`extractionProfile` | { [key: string]: any; }
`extractionResult` | { [key: string]: any; }
`proofBundle` | { [key: string]: any; }
`targetResolution` | { [key: string]: any; }
`telemetry` | { [key: string]: any; } — renderer-scoped browser telemetry; missing resource/cost fields are unknown rather than zero.
`observedAt` | Date

## Example

```typescript
import type { BrowserObservation } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "step": null,
  "url": null,
  "title": null,
  "snapshot": null,
  "domSummary": null,
  "screenshotArtifactId": null,
  "visualObservationArtifactId": null,
  "evidenceDeltaArtifactId": null,
  "consoleSummary": null,
  "networkSummary": null,
  "egressReceipts": null,
  "dialogs": null,
  "policyDenials": null,
  "actionOutcome": null,
  "observationDelta": null,
  "challenge": null,
  "extractionProfile": null,
  "extractionResult": null,
  "proofBundle": null,
  "targetResolution": null,
  "telemetry": {},
  "observedAt": null,
} satisfies BrowserObservation

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserObservation
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)
