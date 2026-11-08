
# Source


## Properties

Name | Type
------------ | -------------
`sourceId` | string
`orgId` | string
`name` | string
`url` | string
`kind` | string
`status` | string
`createdAt` | Date
`updatedAt` | Date
`config` | any

## Example

```typescript
import type { Source } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "sourceId": null,
  "orgId": null,
  "name": null,
  "url": null,
  "kind": null,
  "status": null,
  "createdAt": null,
  "updatedAt": null,
  "config": null,
} satisfies Source

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Source
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


