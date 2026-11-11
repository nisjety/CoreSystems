
# CreateSourceRequestConfig

The source\'s config. A saved job definition (E4.1-6) sits at config.definition: the edge checks it and stores the canonical form (see POST /v1/sources and SavedJobDefinition). Other keys are kept as sent.

## Properties

Name | Type
------------ | -------------
`definition` | [SavedJobDefinition](SavedJobDefinition.md)

## Example

```typescript
import type { CreateSourceRequestConfig } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "definition": null,
} satisfies CreateSourceRequestConfig

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CreateSourceRequestConfig
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


