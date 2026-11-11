
# CrawlRequestScrapeOptions

INNH-6b. The per-page extraction options, applied to every page the crawl fetches. Dark: with QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS off the field is a 400 `unsupported_option`. The same validators as /v1/scrape; the same limits (32 selectors of 256 characters, `timeout_ms` 1000 to 55000) and the same camelCase aliases. Only the options that can apply per page are accepted: `actions`, `actions_on_error`, `max_inline_bytes`, `screenshot`, `pdf`, `json`, `summary`, `question`, `highlights`, `locale`, `timezone`, `location`, `profile_id` and `profileId` (a browser set-up or profile for one request, which a crawl page does not have) and capture or AI `formats` are a 400 `unsupported_option` with the path (`scrape_options.actions`). The stored markdown and html artifacts of each page, and the text `ingest: true` sends to the Data Plane, are the cleaned content. Links are discovered from the whole page whatever `include_tags` and `exclude_tags` cut. A page cleaned differently from the default neither reads nor writes the page cache. `redact_pii: on` redacts each page\'s text before anything is stored, through the pipeline stage. It needs QUARRY_EDGE__PII_REDACTION on; off, it is a 400 `unsupported_option` (path `scrape_options.redact_pii`). A redacted page neither reads nor writes the page cache. `data.effective.scrape_options` echoes the resolved form.

## Properties

Name | Type
------------ | -------------
`formats` | Array&lt;string&gt;
`onlyMainContent` | boolean
`includeTags` | Array&lt;string&gt;
`excludeTags` | Array&lt;string&gt;
`removeBase64Images` | boolean
`extractor` | string
`timeoutMs` | number

## Example

```typescript
import type { CrawlRequestScrapeOptions } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "formats": null,
  "onlyMainContent": null,
  "includeTags": null,
  "excludeTags": null,
  "removeBase64Images": null,
  "extractor": null,
  "timeoutMs": null,
} satisfies CrawlRequestScrapeOptions

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlRequestScrapeOptions
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


