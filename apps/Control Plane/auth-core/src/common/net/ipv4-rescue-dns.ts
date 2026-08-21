import * as dns from 'node:dns';
import type { LookupAddress } from 'node:dns';
import { Agent, setGlobalDispatcher } from 'undici';

/**
 * Rescue lookup for Node's global fetch on Alpine/musl behind Docker's
 * embedded resolver.
 *
 * musl's getaddrinfo fires the A and AAAA queries in parallel and, for hosts
 * whose A answer is very large (login.microsoftonline.com's CNAME chain), the
 * A response is intermittently lost — getaddrinfo then returns IPv6-only
 * results in a container with no IPv6 route. Node's fetch ENETUNREACHes every
 * address, and Better Auth surfaces the failed Microsoft token exchange as
 * `invalid_code`, locking users out of sign-in. Confirmed empirically:
 * `dns.lookup` returned 48 AAAA / 0 A for that host while `dns.resolve4`
 * (c-ares, same resolver) returned the full A set, and a raw IPv4 connect to
 * one of those addresses succeeded.
 *
 * The rescue keeps normal getaddrinfo semantics (so /etc/hosts entries and
 * service aliases behave exactly as before) and only when a result contains
 * no IPv4 address augments it with a direct A-record resolution, IPv4 first.
 */
// dns.lookup's option parameter across its overloads: a family number or an
// options object. Deriving this via Parameters<typeof dns.lookup> collapses
// to `never` under the overload set, so it is spelled out instead.
type LookupOptions = number | (dns.LookupOptions & { all?: boolean }) | undefined;
type LookupCallback = (
  err: NodeJS.ErrnoException | null,
  address: string | LookupAddress[],
  family?: number,
) => void;

function ipv4RescueLookup(
  hostname: string,
  options: LookupOptions,
  callback: LookupCallback,
): void {
  const wantAll = Boolean(
    options && typeof options === 'object' && options.all,
  );

  const deliver = (addresses: LookupAddress[]): void => {
    if (wantAll) {
      callback(null, addresses);
      return;
    }
    const first = addresses[0];
    callback(null, first?.address ?? '', first?.family);
  };

  const baseOptions = typeof options === 'object' && options ? options : {};
  dns.lookup(hostname, { ...baseOptions, all: true }, (err, result) => {
    const addresses = Array.isArray(result) ? result : [];
    const hasIpv4 = addresses.some((entry) => entry.family === 4);
    if (!err && hasIpv4) {
      deliver(addresses);
      return;
    }

    dns.resolve4(hostname, (rescueErr, ipv4) => {
      if (rescueErr || ipv4.length === 0) {
        // Nothing to rescue with — preserve the original outcome verbatim.
        if (err) {
          callback(err, addresses);
        } else {
          deliver(addresses);
        }
        return;
      }
      const rescued: LookupAddress[] = [
        ...ipv4.map((address) => ({ address, family: 4 as const })),
        ...addresses.filter((entry) => entry.family === 6),
      ];
      deliver(rescued);
    });
  });
}

/**
 * Install the rescue lookup as the connector for Node's global fetch. undici
 * registers its global dispatcher under a `Symbol.for` key, so an Agent set
 * here is picked up by the built-in fetch that Better Auth uses.
 */
export function installIpv4RescueDns(): void {
  setGlobalDispatcher(
    new Agent({
      connect: { lookup: ipv4RescueLookup as never },
    }),
  );
}
