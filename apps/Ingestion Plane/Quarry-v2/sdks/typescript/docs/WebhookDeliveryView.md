
# WebhookDeliveryView

One delivery of an event to a subscription. No payload body.

## Properties

Name | Type
------------ | -------------
`id` | string
`eventId` | string
`eventType` | string
`status` | string
`attempt` | number
`nextAttemptAt` | Date
`lastError` | string
`createdAt` | Date

## Example

```typescript
import type { WebhookDeliveryView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "eventId": null,
  "eventType": null,
  "status": null,
  "attempt": null,
  "nextAttemptAt": null,
  "lastError": null,
  "createdAt": null,
} satisfies WebhookDeliveryView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as WebhookDeliveryView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


