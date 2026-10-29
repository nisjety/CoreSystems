
# TeamTokenUsage


## Properties

Name | Type
------------ | -------------
`orgId` | string
`period` | string
`inputTokens` | number
`outputTokens` | number
`totalTokens` | number
`costMicroUsd` | number

## Example

```typescript
import type { TeamTokenUsage } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "orgId": null,
  "period": null,
  "inputTokens": null,
  "outputTokens": null,
  "totalTokens": null,
  "costMicroUsd": null,
} satisfies TeamTokenUsage

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TeamTokenUsage
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


