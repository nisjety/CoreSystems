
# RedactionStamp

What the secret-redaction pass did to a record. It carries counts per static rule id and no content and no lengths. Every field has a default, so a stamp written by an older build still reads.

## Properties

Name | Type
------------ | -------------
`ruleVersion` | string
`profile` | string
`mode` | string
`valuesMasked` | number
`byRule` | { [key: string]: number; }
`probe` | string
`probeMs` | number
`screenshot` | string
`maskMs` | number
`oopifFrames` | number
`rects` | number

## Example

```typescript
import type { RedactionStamp } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "ruleVersion": null,
  "profile": null,
  "mode": null,
  "valuesMasked": null,
  "byRule": null,
  "probe": null,
  "probeMs": null,
  "screenshot": null,
  "maskMs": null,
  "oopifFrames": null,
  "rects": null,
} satisfies RedactionStamp

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RedactionStamp
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


