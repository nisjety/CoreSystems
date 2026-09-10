import { RemoteConnectionError } from '../errors/RemoteError.js';

/**
 * `ConnectOptions.signal` var tidligere tatt imot og ignorert. Disse to
 * hjelperne er det som gjør den reell: en protokollimplementasjon må kjøre
 * HVER ventende operasjon i tilkoblingssekvensen gjennom `withAbort`, ellers
 * fortsetter forsøket etter at kalleren har avbrutt — og en `signal` som ikke
 * gjør noe er verre enn ingen `signal`, siden kalleren tror den har kansellert.
 */
export function abortError(): RemoteConnectionError {
  return new RemoteConnectionError('The connection attempt was aborted by the caller', {
    reason: 'user',
  });
}

export function throwIfAborted(signal: AbortSignal | undefined): void {
  if (signal?.aborted === true) throw abortError();
}

/**
 * Løser som `promise`, men avviser straks `signal` avbrytes. Merk at det
 * underliggende arbeidet IKKE stanses — det finnes ingen generisk måte å
 * avbryte et vilkårlig løfte. Kalleren (runConnect → abandonConnect) rydder
 * opp ressursene etterpå; dette gir bare et raskt, typet avbrudd.
 */
export async function withAbort<T>(promise: Promise<T>, signal: AbortSignal | undefined): Promise<T> {
  if (!signal) return promise;
  if (signal.aborted) throw abortError();

  let onAbort: (() => void) | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        onAbort = () => reject(abortError());
        signal.addEventListener('abort', onAbort, { once: true });
      }),
    ]);
  } finally {
    if (onAbort) signal.removeEventListener('abort', onAbort);
  }
}

/**
 * Avviser med `error` hvis `promise` ikke er ferdig innen `timeoutMs`. Brukes
 * på ventinger som IKKE går gjennom MessageInbox (som selv har tidsavbrudd) —
 * i praksis tofaktor-forespørselen, der vi venter på et menneske.
 */
export async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  error: () => Error,
  scheduleTimeout: (handler: () => void, ms: number) => unknown = setTimeout,
  clearScheduled: (handle: unknown) => void = (handle) => clearTimeout(handle as never),
): Promise<T> {
  if (timeoutMs <= 0 || !Number.isFinite(timeoutMs)) return promise;

  let handle: unknown;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        handle = scheduleTimeout(() => reject(error()), timeoutMs);
      }),
    ]);
  } finally {
    if (handle !== undefined) clearScheduled(handle);
  }
}
