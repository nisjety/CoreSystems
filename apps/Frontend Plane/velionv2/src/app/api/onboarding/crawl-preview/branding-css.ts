const VENDOR_CUSTOM_PROPERTY_PREFIXES = [
  "wc-",
  "wp-",
  "yith-",
  "woocommerce-",
  "cf7-",
  "complianz-",
  "mycred-",
];

const BRAND_PROPERTY_NAME = /(?:brand|primary|accent|color-primary|color-brand)[\w-]*/i;

export function prioritizeStylesheetUrls(urls: string[], pageUrl: string): string[] {
  const page = new URL(pageUrl);

  return urls
    .map((url, index) => ({ url, index, score: stylesheetScore(url, page) }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((item) => item.url);
}

export function extractCustomPropertyColors(css: string, cssColorToHex: (value: string | null | undefined) => string | null): string[] {
  const colors: string[] = [];

  for (const match of css.matchAll(/--([\w-]+)\s*:\s*([^;}]+)/gi)) {
    const name = match[1].toLowerCase();
    if (!BRAND_PROPERTY_NAME.test(name)) continue;
    if (isVendorCustomProperty(name)) continue;

    const color = cssColorToHex(match[2]);
    if (color) colors.push(color);
  }

  return uniqueStrings(colors);
}

function stylesheetScore(input: string, page: URL): number {
  let parsed: URL;
  try {
    parsed = new URL(input);
  } catch {
    return -100;
  }

  const path = parsed.pathname.toLowerCase();
  const hostSlug = page.hostname.replace(/^www\./, "").split(".")[0]?.toLowerCase() ?? "";
  let score = 0;

  if (parsed.origin === page.origin) score += 20;
  if (hostSlug && path.includes(hostSlug)) score += 35;
  if (path.includes("/wp-content/themes/")) score += 50;
  if (path.includes("/themes/")) score += 35;
  if (/(^|\/)(style|theme|main|app|site|global|bundle)\.css$/.test(path)) score += 18;
  if (path.includes("/assets/") || path.includes("/css/")) score += 6;

  if (path.includes("/wp-content/plugins/")) score -= 45;
  if (path.includes("/plugins/")) score -= 35;
  if (/woocommerce|yith|wishlist|vipps|stripe|payment|cookie|complianz|contact-form|mycred|fontawesome|brands/.test(path)) {
    score -= 35;
  }
  if (/fonts\.googleapis\.com|google|cdn|analytics/.test(parsed.hostname.toLowerCase())) score -= 25;

  return score;
}

function isVendorCustomProperty(name: string): boolean {
  return VENDOR_CUSTOM_PROPERTY_PREFIXES.some((prefix) => name.startsWith(prefix));
}

function uniqueStrings(values: Array<string | undefined | null>): string[] {
  return Array.from(new Set(values.filter((value): value is string => Boolean(value))));
}
