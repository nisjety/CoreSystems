
# AgentStepRequest


## Properties

Name | Type
------------ | -------------
`action` | { [key: string]: any; }
`instruction` | string
`extractionProfile` | { [key: string]: any; }

## Example

```typescript
import type { AgentStepRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "action": null,
  "instruction": null,
  "extractionProfile": null,
} satisfies AgentStepRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AgentStepRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


