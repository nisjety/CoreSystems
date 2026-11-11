
# WebhookSubscriptionCreated


## Properties

Name | Type
------------ | -------------
`id` | string
`url` | string
`events` | Array&lt;string&gt;
`active` | boolean
`description` | string
`notifyOnChange` | [NotifyOnChangeFilter](NotifyOnChangeFilter.md)
`secretHint` | string
`createdAt` | Date
`lastDelivery` | [WebhookLastDelivery](WebhookLastDelivery.md)
`secret` | string

## Example

```typescript
import type { WebhookSubscriptionCreated } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "url": null,
  "events": null,
  "active": null,
  "description": null,
  "notifyOnChange": null,
  "secretHint": null,
  "createdAt": null,
  "lastDelivery": null,
  "secret": null,
} satisfies WebhookSubscriptionCreated

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as WebhookSubscriptionCreated
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


