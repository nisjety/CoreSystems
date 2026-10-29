import { readFileSync } from 'node:fs';

const FALLBACK_DOMAINS = [
  '10minutemail.com',
  'guerrillamail.com',
  'mailinator.com',
  'tempmail.com',
  'throwawaymail.com',
  'yopmail.com',
];

type DomainCache = {
  filePath: string | undefined;
  domains: ReadonlySet<string>;
};

let cache: DomainCache | undefined;

export class DisposableEmailError extends Error {
  constructor(domain: string) {
    super(`Disposable email domains are not allowed: ${domain}`);
    this.name = 'DisposableEmailError';
  }
}

export function emailDomain(email: string): string | undefined {
  const [, domain] = email.trim().toLowerCase().split('@');
  if (!domain) return undefined;
  return domain.replace(/\.$/, '');
}

function blocklistEnabled(): boolean {
  return (
    (process.env.DISPOSABLE_EMAIL_BLOCKLIST_ENABLED ?? 'true').toLowerCase() !==
    'false'
  );
}

function parseDomainList(content: string): string[] {
  return content
    .split(/\r?\n/)
    .map((line) => line.trim().toLowerCase())
    .filter((line) => line && !line.startsWith('#'))
    .map((line) => line.replace(/\.$/, ''));
}

function readConfiguredDomains(filePath: string | undefined): string[] {
  if (!filePath) return [];
  try {
    return parseDomainList(readFileSync(filePath, 'utf8'));
  } catch {
    return [];
  }
}

function loadDomains(): ReadonlySet<string> {
  const filePath =
    process.env.DISPOSABLE_EMAIL_DOMAINS_FILE?.trim() || undefined;
  if (cache && cache.filePath === filePath) {
    return cache.domains;
  }

  const configuredDomains = readConfiguredDomains(filePath);
  const domains = new Set(
    configuredDomains.length > 0 ? configuredDomains : FALLBACK_DOMAINS,
  );
  cache = { filePath, domains };
  return domains;
}

export function isDisposableEmail(email: string): boolean {
  if (!blocklistEnabled()) return false;
  const domain = emailDomain(email);
  if (!domain) return false;
  return loadDomains().has(domain);
}

export function assertNotDisposableEmail(email: string): void {
  const domain = emailDomain(email);
  if (domain && isDisposableEmail(email)) {
    throw new DisposableEmailError(domain);
  }
}

export function resetDisposableEmailDomainCacheForTest(): void {
  cache = undefined;
}
