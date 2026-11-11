
# SuggestEntityPanel


## Properties

Name | Type
------------ | -------------
`name` | string
`kind` | string
`summary` | string
`facts` | [Array&lt;SuggestEntityFact&gt;](SuggestEntityFact.md)

## Example

```typescript
import type { SuggestEntityPanel } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "name": null,
  "kind": null,
  "summary": null,
  "facts": null,
} satisfies SuggestEntityPanel

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SuggestEntityPanel
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


