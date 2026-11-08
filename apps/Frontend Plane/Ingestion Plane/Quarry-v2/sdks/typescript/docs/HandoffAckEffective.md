
# HandoffAckEffective

WS14.S3. Set by /v1/crawl only: the scope and bounds the crawl runs with. `scope_mode` is observe (count, drop nothing; the default) or enforce; `legacy` is true when the scope switch is off; `warnings` lists the widened switches. Never a URL.

## Properties

Name | Type
------------ | -------------
`scope` | object
`scopeMode` | string
`robots` | string
`maxPages` | number
`maxDepth` | number
`legacy` | boolean
`warnings` | Array&lt;string&gt;

## Example

```typescript
import type { HandoffAckEffective } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "scope": null,
  "scopeMode": null,
  "robots": null,
  "maxPages": null,
  "maxDepth": null,
  "legacy": null,
  "warnings": null,
} satisfies HandoffAckEffective

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as HandoffAckEffective
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


