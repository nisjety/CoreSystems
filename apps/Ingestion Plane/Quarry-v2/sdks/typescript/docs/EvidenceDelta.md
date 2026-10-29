
# EvidenceDelta

Bounded DOM, visual, and redacted-network evidence stored in an artifact.

## Properties

Name | Type
------------ | -------------
`version` | number
`step` | number
`dom` | [EvidenceDeltaDom](EvidenceDeltaDom.md)
`network` | [EvidenceDeltaNetwork](EvidenceDeltaNetwork.md)
`visualObservationArtifactId` | string

## Example

```typescript
import type { EvidenceDelta } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "version": null,
  "step": null,
  "dom": null,
  "network": null,
  "visualObservationArtifactId": null,
} satisfies EvidenceDelta

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as EvidenceDelta
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


