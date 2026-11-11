
# SourceCreatedAt

Control writes this as Unix milliseconds. GET /v1/sources answers it as an RFC 3339 string, while the single-source read and the create, tag and definition routes pass control\'s milliseconds through, so a client accepts either.

## Properties

Name | Type
------------ | -------------

## Example

```typescript
import type { SourceCreatedAt } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
} satisfies SourceCreatedAt

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SourceCreatedAt
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


