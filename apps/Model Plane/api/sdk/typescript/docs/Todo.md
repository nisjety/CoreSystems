
# Todo


## Properties

Name | Type
------------ | -------------
`id` | string
`threadId` | string
`runId` | string
`assignee` | string
`title` | string
`description` | string
`state` | [TodoState](TodoState.md)
`priority` | string
`blockedBy` | Array&lt;string&gt;

## Example

```typescript
import type { Todo } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "threadId": null,
  "runId": null,
  "assignee": null,
  "title": null,
  "description": null,
  "state": null,
  "priority": null,
  "blockedBy": null,
} satisfies Todo

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Todo
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


