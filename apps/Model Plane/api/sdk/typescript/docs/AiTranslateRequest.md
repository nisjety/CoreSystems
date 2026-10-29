
# AiTranslateRequest


## Properties

Name | Type
------------ | -------------
`text` | string
`sourceLanguage` | string
`targetLanguage` | string

## Example

```typescript
import type { AiTranslateRequest } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "text": null,
  "sourceLanguage": null,
  "targetLanguage": null,
} satisfies AiTranslateRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiTranslateRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


