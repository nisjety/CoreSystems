
# SourceDefinitionRequest

The body of PUT /v1/sources/{source_id}/definition. definition replaces the source\'s saved job definition, and null clears it. The key is required: a body that names no definition is a 400 (details.fields[] code required), so a PUT never clears a definition by omission.

## Properties

Name | Type
------------ | -------------
`definition` | [SavedJobDefinition](SavedJobDefinition.md)

## Example

```typescript
import type { SourceDefinitionRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "definition": null,
} satisfies SourceDefinitionRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SourceDefinitionRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


