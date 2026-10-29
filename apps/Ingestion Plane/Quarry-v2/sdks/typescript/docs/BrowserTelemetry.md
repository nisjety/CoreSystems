
# BrowserTelemetry

Measured browser execution facts. renderer_* values are renderer-scoped CDP metrics, not host-wide process usage. Missing cost/resource values mean no provider-authoritative meter was available.

## Properties

Name | Type
------------ | -------------
`startupMode` | string
`startupLatencyMs` | number
`usableObservationLatencyMs` | number
`rendererTaskCpuMs` | number
`rendererJsHeapUsedBytes` | number
`estimatedSnapshotTokens` | number
`observedActionCount` | number
`challengeObservationCount` | number
`challengeRatePerMille` | number
`verifiedActionCostMicroUsd` | number

## Example

```typescript
import type { BrowserTelemetry } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "startupMode": null,
  "startupLatencyMs": null,
  "usableObservationLatencyMs": null,
  "rendererTaskCpuMs": null,
  "rendererJsHeapUsedBytes": null,
  "estimatedSnapshotTokens": null,
  "observedActionCount": null,
  "challengeObservationCount": null,
  "challengeRatePerMille": null,
  "verifiedActionCostMicroUsd": null,
} satisfies BrowserTelemetry

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserTelemetry
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


