
# TransitionTodoBody


## Properties

Name | Type
------------ | -------------
`status` | [TodoState](TodoState.md)
`reason` | string

## Example

```typescript
import type { TransitionTodoBody } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "reason": null,
} satisfies TransitionTodoBody

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TransitionTodoBody
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


