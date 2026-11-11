
# ClearProfileData200Response


## Properties

Name | Type
------------ | -------------
`profileId` | string
`dryRun` | boolean
`cleared` | boolean
`counts` | [ClearProfileData200ResponseCounts](ClearProfileData200ResponseCounts.md)

## Example

```typescript
import type { ClearProfileData200Response } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "profileId": null,
  "dryRun": null,
  "cleared": null,
  "counts": null,
} satisfies ClearProfileData200Response

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ClearProfileData200Response
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


