
# ScrapeRequestLocation

E4.4-a. The geolocation the page reports to navigator.geolocation, in decimal degrees, with an accuracy in metres (default 100). The position is granted to the origin the scrape navigates to, which is the permission a site checks before it reads the position. The same switch, cache and stream rules as locale. 400 out_of_range with path location.latitude, location.longitude or location.accuracy; missing_field or unknown_field inside the object; invalid_type for a value that is not a number or an object.

## Properties

Name | Type
------------ | -------------
`latitude` | number
`longitude` | number
`accuracy` | number

## Example

```typescript
import type { ScrapeRequestLocation } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "latitude": null,
  "longitude": null,
  "accuracy": null,
} satisfies ScrapeRequestLocation

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeRequestLocation
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


