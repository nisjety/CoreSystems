
# ProcedureImpactRequest


## Properties

Name | Type
------------ | -------------
`procedure` | [BrowserProcedure](BrowserProcedure.md)
`changedUrls` | Array&lt;string&gt;

## Example

```typescript
import type { ProcedureImpactRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "procedure": null,
  "changedUrls": null,
} satisfies ProcedureImpactRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProcedureImpactRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


