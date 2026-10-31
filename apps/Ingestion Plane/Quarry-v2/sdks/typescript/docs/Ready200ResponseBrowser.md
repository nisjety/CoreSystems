
# Ready200ResponseBrowser

A local browser driver\'s health. Counts and states only, no session data.

## Properties

Name | Type
------------ | -------------
`state` | string
`maxSessions` | number
`sessionsOpen` | number
`slots` | [Array&lt;Ready200ResponseBrowserSlotsInner&gt;](Ready200ResponseBrowserSlotsInner.md)

## Example

```typescript
import type { Ready200ResponseBrowser } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "state": null,
  "maxSessions": null,
  "sessionsOpen": null,
  "slots": null,
} satisfies Ready200ResponseBrowser

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Ready200ResponseBrowser
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


