
# ProfileSummary


## Properties

Name | Type
------------ | -------------
`profileId` | string
`name` | string
`scope` | string
`ownerIsCaller` | boolean

## Example

```typescript
import type { ProfileSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "profileId": null,
  "name": null,
  "scope": null,
  "ownerIsCaller": null,
} satisfies ProfileSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProfileSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


