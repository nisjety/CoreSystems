
# ProcedureSlot

Version 2 only: a named input a procedure leaves open. The step types into it at replay; the value is supplied then and is never stored in the procedure. 

## Properties

Name | Type
------------ | -------------
`name` | string
`step` | number
`kind` | string

## Example

```typescript
import type { ProcedureSlot } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "name": null,
  "step": null,
  "kind": null,
} satisfies ProcedureSlot

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProcedureSlot
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


