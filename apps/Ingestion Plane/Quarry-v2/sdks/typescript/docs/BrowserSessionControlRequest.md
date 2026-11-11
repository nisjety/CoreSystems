
# BrowserSessionControlRequest

Who should drive the run\'s browser. Only a person may send it.

## Properties

Name | Type
------------ | -------------
`mode` | string
`expectedEpoch` | number

## Example

```typescript
import type { BrowserSessionControlRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "mode": null,
  "expectedEpoch": null,
} satisfies BrowserSessionControlRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserSessionControlRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


