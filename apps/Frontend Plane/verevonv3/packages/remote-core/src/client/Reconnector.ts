import type { Logger } from '../logging/Logger.js';
import { AuthenticationError, PermissionDeniedError } from '../errors/RemoteError.js';
import { withTimeout } from '../protocol/abort.js';

export interface ReconnectPolicy {
  /** Antall nye tilkoblingsforsøk før vi gir opp. 0 = aldri prøv på nytt. */
  readonly maxAttempts: number;
  readonly baseDelayMs: number;
  readonly maxDelayMs: number;
  /**
   * Frist per forsøk. Uten den kan ETT forsøk henge for alltid (et
   * tilkoblingsforsøk kan vente på ting som ikke har eget tidsavbrudd), og da
   * står økten i 'reconnecting' i det uendelige uten noen gang å feile. 0 =
   * ingen frist. Standard: 45 s.
   */
  readonly attemptTimeoutMs?: number;
}

export const DEFAULT_RECONNECT_POLICY: ReconnectPolicy = {
  maxAttempts: 5,
  baseDelayMs: 500,
  maxDelayMs: 8_000,
  attemptTimeoutMs: 45_000,
};

/**
 * Utfallet er bevisst et diskriminert union og ikke en boolsk verdi:
 * «avbrutt», «oppbrukt» og «avvist» må kunne rapporteres forskjellig, ellers
 * ender en brukerinitiert frakobling opp som «ga opp etter 5 forsøk».
 */
export type ReconnectOutcome =
  | { readonly kind: 'succeeded'; readonly attempts: number }
  | { readonly kind: 'cancelled' }
  | { readonly kind: 'exhausted'; readonly attempts: number; readonly lastError: Error | undefined }
  /** Et forsøk feilet på en måte flere forsøk ikke kan løse (feil legitimasjon, tilbakekalt tilgang). */
  | { readonly kind: 'refused'; readonly error: Error };

export interface ReconnectorOptions {
  readonly policy: ReconnectPolicy;
  /** Gjør ETT fullt tilkoblingsforsøk; avviser ved feil. */
  readonly attempt: () => Promise<void>;
  readonly logger: Logger;
  /** Injiseres i tester for deterministisk backoff. */
  readonly sleep?: (ms: number) => Promise<void>;
  /** Injiseres i tester sammen med `clearScheduled` for en deterministisk forsøksfrist. */
  readonly scheduleTimeout?: (handler: () => void, ms: number) => unknown;
  readonly clearScheduled?: (handle: unknown) => void;
}

/**
 * Å prøve igjen kan ikke hjelpe mot disse: legitimasjonen er feil, eller
 * tilgangen er trukket. Da er fem forsøk bare fem like avvisninger — og for
 * en tofaktor-vert også fem kodedialoger.
 */
function isRefusal(error: unknown): error is Error {
  return error instanceof AuthenticationError || error instanceof PermissionDeniedError;
}

/**
 * Eksponentiell backoff rundt et gjenoppkoblingsforsøk. Holdes atskilt fra
 * RemoteSession slik at selve policyen (hvor mange ganger, hvor lenge) er
 * testbar uten en tilstandsmaskin eller protokoll i veien.
 *
 * `cancel()` er ikke en hard avbrytelse av et pågående forsøk — den hindrer
 * at NESTE forsøk starter, som er det som trengs når brukeren kobler fra
 * eksplisitt midt i en gjenoppkobling.
 */
export class Reconnector {
  private cancelled = false;
  private running = false;

  constructor(private readonly options: ReconnectorOptions) {}

  get isRunning(): boolean {
    return this.running;
  }

  cancel(): void {
    this.cancelled = true;
  }

  async run(): Promise<ReconnectOutcome> {
    if (this.running) return { kind: 'cancelled' };
    this.running = true;
    this.cancelled = false;
    const sleep = this.options.sleep ?? defaultSleep;
    const { maxAttempts, baseDelayMs, maxDelayMs } = this.options.policy;
    const attemptTimeoutMs = this.options.policy.attemptTimeoutMs ?? 0;
    let lastError: Error | undefined;

    try {
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        const delay = Math.min(maxDelayMs, baseDelayMs * 2 ** (attempt - 1));
        await sleep(delay);
        if (this.cancelled) return { kind: 'cancelled' };

        try {
          await this.runAttempt(attemptTimeoutMs);
          this.options.logger.info('Reconnected', { attempt });
          return { kind: 'succeeded', attempts: attempt };
        } catch (error) {
          lastError = error instanceof Error ? error : new Error(String(error));
          this.options.logger.warn('Reconnect attempt failed', {
            attempt,
            maxAttempts,
            error: lastError.message,
          });
          if (this.cancelled) return { kind: 'cancelled' };
          if (isRefusal(error)) {
            return { kind: 'refused', error: lastError };
          }
        }
      }
      return { kind: 'exhausted', attempts: maxAttempts, lastError };
    } finally {
      this.running = false;
    }
  }

  /**
   * Fristen ryddes når forsøket avgjøres først, så et vellykket forsøk ikke
   * etterlater en tidtaker som tikker videre i bakgrunnen. Merk at et forsøk
   * som brytes på tid ikke stanses — det finnes ingen generisk måte å avbryte
   * det — men løkken får gå videre i stedet for å stå fast for alltid.
   */
  private runAttempt(attemptTimeoutMs: number): Promise<void> {
    return withTimeout(
      this.options.attempt(),
      attemptTimeoutMs,
      () => new Error(`Reconnect attempt exceeded ${attemptTimeoutMs} ms`),
      this.options.scheduleTimeout ?? setTimeout,
      this.options.clearScheduled ?? ((handle) => clearTimeout(handle as never)),
    );
  }
}

function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
