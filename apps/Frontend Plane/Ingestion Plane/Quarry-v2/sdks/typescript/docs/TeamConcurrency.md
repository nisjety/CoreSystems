
# TeamConcurrency


## Properties

Name | Type
------------ | -------------
`orgId` | string
`current` | number
`ceiling` | number
`byHost` | [Array&lt;HostConcurrency&gt;](HostConcurrency.md)

## Example

```typescript
import type { TeamConcurrency } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "orgId": null,
  "current": null,
  "ceiling": null,
  "byHost": null,
} satisfies TeamConcurrency

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TeamConcurrency
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


