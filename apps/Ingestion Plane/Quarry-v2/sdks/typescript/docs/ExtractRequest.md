
# ExtractRequest


## Properties

Name | Type
------------ | -------------
`urls` | Array&lt;string&gt;
`schema` | object
`prompt` | string
`maxUrls` | number

## Example

```typescript
import type { ExtractRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "urls": null,
  "schema": null,
  "prompt": null,
  "maxUrls": null,
} satisfies ExtractRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ExtractRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


