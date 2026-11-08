
# SnapshotTarget


## Properties

Name | Type
------------ | -------------
`refId` | string
`tag` | string
`text` | string
`role` | string
`name` | string
`placeholder` | string
`testId` | string
`frameId` | string
`fingerprint` | [ElementFingerprint](ElementFingerprint.md)

## Example

```typescript
import type { SnapshotTarget } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "refId": @e1,
  "tag": null,
  "text": null,
  "role": null,
  "name": null,
  "placeholder": null,
  "testId": null,
  "frameId": null,
  "fingerprint": null,
} satisfies SnapshotTarget

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SnapshotTarget
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


