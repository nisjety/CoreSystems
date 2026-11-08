
# ProfileRestoreSummary

What restoring a saved profile into a fresh browser context did. Counts and flags only; never a cookie name, an origin, a URL or a value. The user agent is never overridden; ua_changed only records that it differs from the one the profile was captured with.

## Properties

Name | Type
------------ | -------------
`cookiesTotal` | number
`cookiesApplied` | number
`skippedExpired` | number
`skippedSessionStale` | number
`cookiesRejected` | number
`originsSeeded` | number
`idbDatabases` | number
`uaChanged` | boolean
`snapshotAgeS` | number
`truncated` | boolean
`failed` | boolean

## Example

```typescript
import type { ProfileRestoreSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "cookiesTotal": null,
  "cookiesApplied": null,
  "skippedExpired": null,
  "skippedSessionStale": null,
  "cookiesRejected": null,
  "originsSeeded": null,
  "idbDatabases": null,
  "uaChanged": null,
  "snapshotAgeS": null,
  "truncated": null,
  "failed": null,
} satisfies ProfileRestoreSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProfileRestoreSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


