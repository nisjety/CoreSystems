
# PlanStep


## Properties

Name | Type
------------ | -------------
`id` | string
`title` | string
`operation` | string
`state` | string

## Example

```typescript
import type { PlanStep } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "title": null,
  "operation": null,
  "state": null,
} satisfies PlanStep

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as PlanStep
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


