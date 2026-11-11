
# ScrapeRequest

Fields not listed here are unknown: ignored with an unknown_field warning (and Deprecation and Sunset headers) unless the edge runs strict, where they are a 400. skipTlsVerification, skip_tls_verification, proxy, proxyUrl, headers, mobile and stealth are always a 400 reserved_field. A bad value is a 400 with details.fields[] {path, code, message}.

## Properties

Name | Type
------------ | -------------
`privacy` | object
`render` | [ScrapeRequestRender](ScrapeRequestRender.md)
`preferHttp3` | boolean
`policy` | [RequestAsk](RequestAsk.md)
`url` | string
`prevFingerprint` | string
`cache` | [CachePolicy](CachePolicy.md)
`zdr` | boolean
`signals` | [DriverSignals](DriverSignals.md)
`ingest` | boolean
`orgId` | string
`formats` | [Array&lt;ScrapeRequestFormatsInner&gt;](ScrapeRequestFormatsInner.md)
`onlyMainContent` | boolean
`includeTags` | Array&lt;string&gt;
`excludeTags` | Array&lt;string&gt;
`removeBase64Images` | boolean
`extractor` | string
`timeoutMs` | number
`maxInlineBytes` | number
`actions` | [Array&lt;ScrapeRequestActionsInner&gt;](ScrapeRequestActionsInner.md)
`actionsOnError` | string
`maxAge` | number
`redactPii` | string
`noEgress` | boolean
`locale` | string
`timezone` | string
`location` | [ScrapeRequestLocation](ScrapeRequestLocation.md)
`profileId` | string

## Example

```typescript
import type { ScrapeRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "privacy": null,
  "render": null,
  "preferHttp3": null,
  "policy": null,
  "url": https://example.com/article,
  "prevFingerprint": null,
  "cache": null,
  "zdr": null,
  "signals": null,
  "ingest": null,
  "orgId": null,
  "formats": null,
  "onlyMainContent": null,
  "includeTags": null,
  "excludeTags": null,
  "removeBase64Images": null,
  "extractor": null,
  "timeoutMs": null,
  "maxInlineBytes": null,
  "actions": null,
  "actionsOnError": null,
  "maxAge": null,
  "redactPii": null,
  "noEgress": null,
  "locale": null,
  "timezone": null,
  "location": null,
  "profileId": null,
} satisfies ScrapeRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


