
# InternalRunPageResult


## Properties

Name | Type
------------ | -------------
`runId` | string
`status` | number
`fingerprint` | string
`links` | Array&lt;string&gt;

## Example

```typescript
import type { InternalRunPageResult } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "status": null,
  "fingerprint": null,
  "links": null,
} satisfies InternalRunPageResult

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as InternalRunPageResult
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


