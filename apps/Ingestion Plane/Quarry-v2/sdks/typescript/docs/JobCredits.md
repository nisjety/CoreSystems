
# JobCredits

Credits a job has used, derived at read time from its fetched-page count (pages fetched x price_per_page). A Quarry-side estimate, not what billing-core invoices: a flat price per fetched page, a browser-rendered page priced like any other, failed and blocked pages free. Present only for page-priced kinds (crawl, batch, scrape, extract) and only while the control service runs with QUARRY_CONTROL__JOB_CREDITS on (default off); absent, never null, otherwise. GET /v1/team/credit-usage sums the same page_fetched events at the same price.

## Properties

Name | Type
------------ | -------------
`used` | number
`unit` | string
`basis` | string
`pricePerPage` | number
`estimated` | boolean
`_final` | boolean

## Example

```typescript
import type { JobCredits } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "used": null,
  "unit": null,
  "basis": null,
  "pricePerPage": null,
  "estimated": null,
  "_final": null,
} satisfies JobCredits

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobCredits
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


