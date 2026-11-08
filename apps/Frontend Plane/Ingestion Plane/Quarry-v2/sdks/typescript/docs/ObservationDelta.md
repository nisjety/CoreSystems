
# ObservationDelta


## Properties

Name | Type
------------ | -------------
`changedFields` | Array&lt;string&gt;
`urlChanged` | boolean
`titleChanged` | boolean
`domChanged` | boolean
`contentChanged` | boolean

## Example

```typescript
import type { ObservationDelta } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "changedFields": null,
  "urlChanged": null,
  "titleChanged": null,
  "domChanged": null,
  "contentChanged": null,
} satisfies ObservationDelta

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ObservationDelta
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


