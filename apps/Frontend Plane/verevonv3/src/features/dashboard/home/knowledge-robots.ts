/**
 * WS14.S4 (X-10): the composer line for pages a crawl refused under robots.txt.
 *
 * `count` is `pages_blocked_robots` from the crawl's terminal event: the part of
 * `pages_blocked` the edge refused because robots.txt disallowed the page, or could
 * not be read under the `disallow` policy. A crawl with no such refusal shows nothing,
 * so the line appears only when the robots file is what kept pages out.
 */
export function robotsBlockedText(
  count: number | undefined,
  tr: (noText: string, enText: string) => string,
): string {
  if (count === undefined || !Number.isFinite(count) || count < 1) return ''
  const pages = Math.floor(count)
  return tr(
    `${pages} ${pages === 1 ? 'side' : 'sider'} blokkert av robots.txt`,
    `${pages} ${pages === 1 ? 'page' : 'pages'} blocked by robots.txt`,
  )
}

/**
 * WS14.S3: the composer line for links a crawl's scope refused.
 *
 * `count` is `links_denied` from the crawl's terminal event. It counts every link the scope
 * refused (duplicates, depth, patterns and robots included), so it can be more than the links
 * that lead off the site. A crawl with no refused link shows nothing.
 */
export function linksDeniedText(
  count: number | undefined,
  tr: (noText: string, enText: string) => string,
): string {
  if (count === undefined || !Number.isFinite(count) || count < 1) return ''
  const links = Math.floor(count)
  return tr(
    `Hoppet over ${links} ${links === 1 ? 'lenke' : 'lenker'} utenfor nettstedet.`,
    `Skipped ${links} ${links === 1 ? 'link' : 'links'} outside this site.`,
  )
}
