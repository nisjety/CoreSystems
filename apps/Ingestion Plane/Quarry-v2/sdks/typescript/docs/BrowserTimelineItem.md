
# BrowserTimelineItem

One item of the owner timeline. kind says which of the other fields it carries, and the rest are absent. Most keys are camelCase, but error_code, tab_id, active_tab_id, event_count and last_sequence are snake_case, as the edge writes them. id is a receipt id for an action item, and a bevt_ id for the others.

## Properties

Name | Type
------------ | -------------
`id` | string
`occurredAt` | Date
`kind` | string
`action` | string
`outcome` | string
`errorCode` | string
`mode` | string
`operation` | string
`tabId` | string
`activeTabId` | string
`holder` | string
`state` | string
`attachId` | string
`initiatedBy` | string
`agent` | [AgentIdentity](AgentIdentity.md)
`controlEpoch` | number
`tabEpoch` | number
`eventCount` | number
`lastSequence` | number

## Example

```typescript
import type { BrowserTimelineItem } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "occurredAt": null,
  "kind": null,
  "action": null,
  "outcome": null,
  "errorCode": null,
  "mode": null,
  "operation": null,
  "tabId": null,
  "activeTabId": null,
  "holder": null,
  "state": null,
  "attachId": null,
  "initiatedBy": null,
  "agent": null,
  "controlEpoch": null,
  "tabEpoch": null,
  "eventCount": null,
  "lastSequence": null,
} satisfies BrowserTimelineItem

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserTimelineItem
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


