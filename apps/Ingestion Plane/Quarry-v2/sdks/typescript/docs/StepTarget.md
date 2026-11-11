
# StepTarget

Version 2 only: the element a ref or semantic step acts on. The locator is the one Quarry resolved the element to. The fingerprint\'s normalized text is never included. 

## Properties

Name | Type
------------ | -------------
`locator` | [SemanticLocator](SemanticLocator.md)
`fingerprintId` | string
`tag` | string

## Example

```typescript
import type { StepTarget } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "locator": null,
  "fingerprintId": null,
  "tag": null,
} satisfies StepTarget

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StepTarget
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


