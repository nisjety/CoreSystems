
# DownloadRefAction


## Properties

Name | Type
------------ | -------------
`type` | string
`snapshotId` | string
`generation` | number
`refId` | string
`approvalGrantId` | string

## Example

```typescript
import type { DownloadRefAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "type": null,
  "snapshotId": null,
  "generation": null,
  "refId": @e1,
  "approvalGrantId": null,
} satisfies DownloadRefAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as DownloadRefAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


