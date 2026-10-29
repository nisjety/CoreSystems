
# ListSchedules200ResponseData


## Properties

Name | Type
------------ | -------------
`items` | [Array&lt;ScheduleSummary&gt;](ScheduleSummary.md)
`nextCursor` | string
`totalEstimated` | number

## Example

```typescript
import type { ListSchedules200ResponseData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "items": null,
  "nextCursor": null,
  "totalEstimated": null,
} satisfies ListSchedules200ResponseData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ListSchedules200ResponseData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


