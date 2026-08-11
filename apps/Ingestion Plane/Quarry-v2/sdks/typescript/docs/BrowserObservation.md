
# BrowserObservation


## Properties

Name | Type
------------ | -------------
`runId` | string
`step` | number
`url` | string
`title` | string
`domSummary` | { [key: string]: any; }
`screenshotArtifactId` | string
`visualObservationArtifactId` | string
`consoleSummary` | Array&lt;{ [key: string]: any; }&gt;
`networkSummary` | Array&lt;{ [key: string]: any; }&gt;
`policyDenials` | Array&lt;string&gt;
`actionOutcome` | [ActionOutcome](ActionOutcome.md)
`observationDelta` | [ObservationDelta](ObservationDelta.md)
`challenge` | { [key: string]: any; }
`extractionProfile` | { [key: string]: any; }
`extractionResult` | { [key: string]: any; }
`proofBundle` | { [key: string]: any; }
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
  "domSummary": null,
  "screenshotArtifactId": null,
  "visualObservationArtifactId": null,
  "consoleSummary": null,
  "networkSummary": null,
  "policyDenials": null,
  "actionOutcome": null,
  "observationDelta": null,
  "challenge": null,
  "extractionProfile": null,
  "extractionResult": null,
  "proofBundle": null,
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


