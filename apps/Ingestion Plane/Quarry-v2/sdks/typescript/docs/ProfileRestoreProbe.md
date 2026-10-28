
# ProfileRestoreProbe


## Properties

Name | Type
------------ | -------------
`profileId` | string
`name` | string
`scope` | string
`url` | string
`restorable` | boolean
`cookiesCount` | number
`localStorageCount` | number
`sessionStorageCount` | number
`indexedDbCount` | number
`earliestExpiry` | Date
`cookiesExpired` | number
`originsCount` | number
`hasUserAgent` | boolean
`hasViewport` | boolean
`locale` | string
`timezone` | string
`ownerIsCaller` | boolean

## Example

```typescript
import type { ProfileRestoreProbe } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "profileId": null,
  "name": null,
  "scope": null,
  "url": null,
  "restorable": null,
  "cookiesCount": null,
  "localStorageCount": null,
  "sessionStorageCount": null,
  "indexedDbCount": null,
  "earliestExpiry": null,
  "cookiesExpired": null,
  "originsCount": null,
  "hasUserAgent": null,
  "hasViewport": null,
  "locale": null,
  "timezone": null,
  "ownerIsCaller": null,
} satisfies ProfileRestoreProbe

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProfileRestoreProbe
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


