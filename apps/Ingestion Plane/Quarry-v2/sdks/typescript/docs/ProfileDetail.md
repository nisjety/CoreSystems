
# ProfileDetail

The data of GET /v1/profiles/{profile_id}. The ProfileView fields are flattened in beside these.

## Properties

Name | Type
------------ | -------------
`cookies` | [Array&lt;ProfileCookieView&gt;](ProfileCookieView.md)
`localStorageCount` | number
`sessionStorageCount` | number
`origins` | [Array&lt;ProfileOriginView&gt;](ProfileOriginView.md)
`locale` | string
`timezone` | string
`hasUserAgent` | boolean
`hasViewport` | boolean
`truncated` | boolean
`profileId` | string
`name` | string
`scope` | string
`ownerIsCaller` | boolean

## Example

```typescript
import type { ProfileDetail } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "cookies": null,
  "localStorageCount": null,
  "sessionStorageCount": null,
  "origins": null,
  "locale": null,
  "timezone": null,
  "hasUserAgent": null,
  "hasViewport": null,
  "truncated": null,
  "profileId": null,
  "name": null,
  "scope": null,
  "ownerIsCaller": null,
} satisfies ProfileDetail

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProfileDetail
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


