
# BrowserSnapshot


## Properties

Name | Type
------------ | -------------
`snapshotId` | string
`generation` | number
`targets` | [Array&lt;SnapshotTarget&gt;](SnapshotTarget.md)
`accessibility` | [AccessibilityProjection](AccessibilityProjection.md)
`frames` | [Array&lt;BrowserFrame&gt;](BrowserFrame.md)

## Example

```typescript
import type { BrowserSnapshot } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "snapshotId": null,
  "generation": null,
  "targets": null,
  "accessibility": null,
  "frames": null,
} satisfies BrowserSnapshot

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserSnapshot
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


