
# ScheduleRefreshRun200Response


## Properties

Name | Type
------------ | -------------
`data` | [ScheduleRefreshRun200ResponseData](ScheduleRefreshRun200ResponseData.md)
`meta` | [ResponseMeta](ResponseMeta.md)
`error` | [ErrorDetail](ErrorDetail.md)

## Example

```typescript
import type { ScheduleRefreshRun200Response } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "data": null,
  "meta": null,
  "error": null,
} satisfies ScheduleRefreshRun200Response

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScheduleRefreshRun200Response
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


