
# WaitForRefAction


## Properties

Name | Type
------------ | -------------
`snapshotId` | string
`generation` | number
`type` | string
`refId` | string
`timeoutMs` | number

## Example

```typescript
import type { WaitForRefAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "snapshotId": null,
  "generation": null,
  "type": null,
  "refId": @e1,
  "timeoutMs": null,
} satisfies WaitForRefAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as WaitForRefAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


