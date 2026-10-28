
# RequestAsk

WS7.C web-policy ask on a scrape request. It can only tighten what the operator baseline and the organisation\'s policy allow; it cannot loosen anything. A domain it denies is a 403 with details.reason policy_denied (capability domains.deny, decision_source request). An unknown capability in disable is a 400 with details.reason unknown_capability. Under QUARRY_EDGE__WEB_POLICY_ENFORCEMENT=observe the ask is only counted, never enforced.

## Properties

Name | Type
------------ | -------------
`disable` | Array&lt;string&gt;
`robotsMax` | string
`proxyMax` | string
`denyDomains` | Array&lt;string&gt;
`allowDomains` | Array&lt;string&gt;

## Example

```typescript
import type { RequestAsk } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "disable": null,
  "robotsMax": null,
  "proxyMax": null,
  "denyDomains": null,
  "allowDomains": null,
} satisfies RequestAsk

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RequestAsk
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


