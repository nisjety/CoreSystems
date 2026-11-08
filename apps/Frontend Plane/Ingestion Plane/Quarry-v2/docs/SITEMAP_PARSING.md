# Sitemap parsing: gzip bodies and robots `Sitemap:` lines (E3.7.4)

Status: pure parsing in `crates/quarry-transform/src/sitemap.rs`, built dark.
INNH-6a calls `parse_body` and the robots `Sitemap:` lines (the parsed `RobotsTxt.sitemaps` of the
robots gate) from `POST /v1/internal/crawl/sitemap` (`crates/quarry-edge/src/crawl_sitemap.rs`),
behind `QUARRY_EDGE__CRAWL_SITEMAP`; see `docs/CONTRACTS.md`, "Crawl `sitemap` and
`scrape_options`". `/v1/map` still reads plain `/sitemap.xml` and its nested indexes only.
The route decodes XML entities and CDATA in `<loc>` itself (`xml_unescape`); `parse` is unchanged.

## What the module does now

| Function | Input | Output |
|---|---|---|
| `parse_body(&[u8])` | fetched sitemap bytes | `Result<Sitemap, SitemapBodyError>` |
| `decode_body(&[u8])` | fetched sitemap bytes | `Result<String, SitemapBodyError>` |
| `sitemap_urls_from_robots(&str)` | robots.txt text | `Vec<String>` of absolute http(s) sitemap URLs |
| `parse(&str)` (unchanged) | sitemap XML text | `Sitemap` |

- A body whose first two bytes are the gzip magic (`1f 8b`) is inflated, whatever
  the URL ends in. Multi-member gzip is read in full.
- The decoded size is capped at `MAX_DECODED_BYTES` (50 MiB, the sitemap
  protocol's single-file cap). The read stops one byte past the cap, so an
  over-long body is reported as `TooLarge`, not cut short silently.
- A corrupt or truncated gzip stream is `Gzip`. Nothing is parsed from it.
- A body without the magic is read as UTF-8, lossily, as before.
- `sitemap_urls_from_robots` follows RFC 9309 section 2.4. The field name is
  case-insensitive, the line is not scoped to a User-agent group, `#` starts a
  comment, a leading BOM is ignored, and CRLF is accepted. Only absolute
  `http` and `https` URLs are returned, normalised, in file order, without
  duplicates. Relative, empty, unparseable and other-scheme values are dropped.

## Rules for the caller (when it is wired)

1. Fetch the sitemap as bytes, not as text. A text fetch turns a gzip body
   into lossy garbage before the parser sees it.
2. Bound the compressed size at the fetch. This module caps the decoded size
   only.
3. Every discovered URL is attacker-controlled. Run preflight and the robots
   check on each one before it is fetched or enqueued.
4. Keep the nested-index fan-out bounded. `/v1/map` takes five nested
   sitemaps today; that cap stays.

## Not done (and why)

- `crates/quarry-edge/src/map_routes.rs`: the sitemap fetch still uses the text
  path. Its hunk waits for W2 U33 and U63 (plan E3.7.4).
- `crates/quarry-transform/src/robots.rs`: owned by W2 U63 (`is_allowed_any`).
  It still collects `Sitemap:` values unvalidated. `sitemap_urls_from_robots`
  is the replacement for U63 to wire.
- Seeding a live crawl from sitemaps: a separate slice (E3.7 discovery half,
  after WS14 S3 and U60).
- XML entity decoding (`&amp;` in `<loc>`) and case-insensitive tag matching in
  `parse`: unchanged in this slice; see the depth file.
