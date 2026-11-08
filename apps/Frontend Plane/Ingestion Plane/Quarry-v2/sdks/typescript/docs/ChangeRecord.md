
# ChangeRecord


## Properties

Name | Type
------------ | -------------
`sourceUrl` | string
`orgId` | string
`status` | string
`newBaseline` | [BaselineSnapshot](BaselineSnapshot.md)
`prevBaseline` | [BaselineSnapshot](BaselineSnapshot.md)
`diffId` | string
`checkedAt` | Date

## Example

```typescript
import type { ChangeRecord } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "sourceUrl": null,
  "orgId": null,
  "status": null,
  "newBaseline": null,
  "prevBaseline": null,
  "diffId": null,
  "checkedAt": null,
} satisfies ChangeRecord

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ChangeRecord
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


