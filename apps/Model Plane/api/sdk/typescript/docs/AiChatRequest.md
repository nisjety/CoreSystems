
# AiChatRequest


## Properties

Name | Type
------------ | -------------
`messages` | [Array&lt;ChatMessage&gt;](ChatMessage.md)
`model` | string
`stream` | boolean
`structuredOutputSchema` | string

## Example

```typescript
import type { AiChatRequest } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "messages": null,
  "model": null,
  "stream": null,
  "structuredOutputSchema": null,
} satisfies AiChatRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiChatRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


