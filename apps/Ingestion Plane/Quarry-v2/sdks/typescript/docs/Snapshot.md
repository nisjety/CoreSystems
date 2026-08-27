
# Snapshot


## Properties

Name | Type
------------ | -------------
`snapshotId` | string
`orgId` | string
`sourceId` | string
`url` | string
`fingerprint` | string
`prevFingerprint` | string
`changeStatus` | string
`capturedAt` | Date
`artifactId` | string

## Example

```typescript
import type { Snapshot } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "snapshotId": null,
  "orgId": null,
  "sourceId": null,
  "url": null,
  "fingerprint": null,
  "prevFingerprint": null,
  "changeStatus": null,
  "capturedAt": null,
  "artifactId": null,
} satisfies Snapshot

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Snapshot
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


