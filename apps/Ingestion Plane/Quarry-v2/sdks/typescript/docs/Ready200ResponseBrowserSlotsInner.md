
# Ready200ResponseBrowserSlotsInner


## Properties

Name | Type
------------ | -------------
`slot` | number
`status` | string
`generation` | number
`sessions` | number
`pages` | number
`healthy` | boolean
`probeFailures` | number

## Example

```typescript
import type { Ready200ResponseBrowserSlotsInner } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "slot": null,
  "status": null,
  "generation": null,
  "sessions": null,
  "pages": null,
  "healthy": null,
  "probeFailures": null,
} satisfies Ready200ResponseBrowserSlotsInner

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Ready200ResponseBrowserSlotsInner
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


