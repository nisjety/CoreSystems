
# AgentAction

Tagged action. Existing CSS selector actions remain supported. Snapshot actions (`*_ref` and `*_semantic`) must include the exact snapshot_id and generation from BrowserObservation.snapshot; stale, missing, or ambiguous targets return TARGET_REPAIR_REQUIRED (409).

## Properties

Name | Type
------------ | -------------
`type` | string
`selector` | string
`x` | number
`y` | number
`snapshotId` | string
`generation` | number
`refId` | string
`locator` | [SemanticLocator](SemanticLocator.md)
`approvalGrantId` | string
`script` | string
`frameId` | string
`value` | string
`text` | string
`artifactId` | string
`timeoutMs` | number
`deltaX` | number
`deltaY` | number
`url` | string
`key` | string
`dialogId` | string
`accept` | boolean
`promptText` | string
`fullPage` | boolean
`target` | string
`ms` | number

## Example

```typescript
import type { AgentAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "type": null,
  "selector": null,
  "x": null,
  "y": null,
  "snapshotId": null,
  "generation": null,
  "refId": @e1,
  "locator": null,
  "approvalGrantId": null,
  "script": null,
  "frameId": null,
  "value": null,
  "text": null,
  "artifactId": null,
  "timeoutMs": null,
  "deltaX": null,
  "deltaY": null,
  "url": null,
  "key": null,
  "dialogId": null,
  "accept": null,
  "promptText": null,
  "fullPage": null,
  "target": null,
  "ms": null,
} satisfies AgentAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AgentAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


