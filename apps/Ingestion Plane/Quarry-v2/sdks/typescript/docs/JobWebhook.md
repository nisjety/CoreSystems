
# JobWebhook

W3-E3.5-3. A per-job webhook: Quarry posts the run\'s events to `url`, signed like a webhook subscription. A bare URL string is also accepted, with the default events. Dark: with QUARRY_EDGE__JOB_WEBHOOK off, `webhook` is a 400 `reserved_field`. The edge checks an https URL with no credentials or fragment, no literal private, loopback or link-local address and no `localhost`, and a closed `events` list; a malformed value is a 400 with `details.fields[]` (path `webhook`, `webhook.url` or `webhook.events`). A zero-data-retention request may not carry one. Control vets the host again before it stores the hook.

## Properties

Name | Type
------------ | -------------
`url` | string
`events` | Array&lt;string&gt;

## Example

```typescript
import type { JobWebhook } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "events": null,
} satisfies JobWebhook

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobWebhook
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


