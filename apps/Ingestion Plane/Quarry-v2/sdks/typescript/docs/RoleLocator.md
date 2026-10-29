
# RoleLocator


## Properties

Name | Type
------------ | -------------
`kind` | string
`role` | string
`name` | string
`exact` | boolean

## Example

```typescript
import type { RoleLocator } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "role": null,
  "name": null,
  "exact": null,
} satisfies RoleLocator

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RoleLocator
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


