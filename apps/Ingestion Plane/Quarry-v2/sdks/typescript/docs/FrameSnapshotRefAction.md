
# FrameSnapshotRefAction

Child-frame action. frame_id must exactly match the observed target\'s opaque frame id; ordinary snapshot refs cannot act across a frame.

## Properties

Name | Type
------------ | -------------
`snapshotId` | string
`generation` | number
`frameId` | string
`type` | string
`refId` | string
`text` | string
`value` | string
`timeoutMs` | number
`artifactId` | string
`approvalGrantId` | string

## Example

```typescript
import type { FrameSnapshotRefAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "snapshotId": null,
  "generation": null,
  "frameId": null,
  "type": null,
  "refId": @e1,
  "text": null,
  "value": null,
  "timeoutMs": null,
  "artifactId": null,
  "approvalGrantId": null,
} satisfies FrameSnapshotRefAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as FrameSnapshotRefAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


