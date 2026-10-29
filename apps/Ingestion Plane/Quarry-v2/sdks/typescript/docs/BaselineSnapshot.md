
# BaselineSnapshot


## Properties

Name | Type
------------ | -------------
`baselineId` | string
`orgId` | string
`sourceUrl` | string
`fingerprint` | string
`artifactId` | string
`prevBaselineId` | string
`capturedAt` | Date
`runId` | string

## Example

```typescript
import type { BaselineSnapshot } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "baselineId": null,
  "orgId": null,
  "sourceUrl": null,
  "fingerprint": null,
  "artifactId": null,
  "prevBaselineId": null,
  "capturedAt": null,
  "runId": null,
} satisfies BaselineSnapshot

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BaselineSnapshot
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


