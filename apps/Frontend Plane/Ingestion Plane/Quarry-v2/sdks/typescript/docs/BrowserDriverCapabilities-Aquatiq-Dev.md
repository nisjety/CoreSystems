
# BrowserDriverCapabilities

Requested or selected execution capabilities. Every true value is an affirmative provider claim; omitted values are not inferred. Agent runs always require isolated_egress and security_evidence regardless of optional requirements.

## Properties

Name | Type
------------ | -------------
`persistentProfile` | boolean
`devtoolsTrace` | boolean
`downloadsToArtifacts` | boolean
`uploadsFromArtifacts` | boolean
`fullVisualFidelity` | boolean
`isolatedEgress` | boolean
`securityEvidence` | boolean
`atomicTargetActions` | boolean

## Example

```typescript
import type { BrowserDriverCapabilities } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "persistentProfile": null,
  "devtoolsTrace": null,
  "downloadsToArtifacts": null,
  "uploadsFromArtifacts": null,
  "fullVisualFidelity": null,
  "isolatedEgress": null,
  "securityEvidence": null,
  "atomicTargetActions": null,
} satisfies BrowserDriverCapabilities

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserDriverCapabilities
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


