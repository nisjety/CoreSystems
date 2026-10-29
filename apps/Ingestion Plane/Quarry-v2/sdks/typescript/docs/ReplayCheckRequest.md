
# ReplayCheckRequest


## Properties

Name | Type
------------ | -------------
`procedure` | [BrowserProcedure](BrowserProcedure.md)
`actions` | Array&lt;{ [key: string]: any; }&gt;

## Example

```typescript
import type { ReplayCheckRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "procedure": null,
  "actions": null,
} satisfies ReplayCheckRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ReplayCheckRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


