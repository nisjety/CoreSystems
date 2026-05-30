
# AiChatResponse


## Properties

Name | Type
------------ | -------------
`id` | string
`content` | string
`modelUsed` | string
`usage` | [AiChatResponseUsage](AiChatResponseUsage.md)

## Example

```typescript
import type { AiChatResponse } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "content": null,
  "modelUsed": null,
  "usage": null,
} satisfies AiChatResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiChatResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


