
# RedirectProbeResult

The answer of POST /v1/browser/redirect-hop. `target` is always present: the absolute URL of a validated redirect, or null for any other status.

## Properties

Name | Type
------------ | -------------
`target` | string

## Example

```typescript
import type { RedirectProbeResult } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "target": null,
} satisfies RedirectProbeResult

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RedirectProbeResult
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


