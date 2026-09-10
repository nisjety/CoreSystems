import { createSignal, For, onCleanup, Show } from 'solid-js';
import type { RemoteDisplay, RemoteSession, SessionState } from '@verevon/remote-core';
import { CanvasRenderer, createRemoteClient } from '@verevon/remote-core';

type DemoState = SessionState | 'idle';

interface SecondFactorRequest {
  readonly resolve: (code: string) => void;
  readonly reject: (error: Error) => void;
}

const STATE_LABELS: Record<DemoState, string> = {
  idle: 'Ikke tilkoblet',
  connecting: 'Kobler til …',
  authenticating: 'Autentiserer …',
  connected: 'Tilkoblet',
  reconnecting: 'Kobler til på nytt …',
  disconnected: 'Tilkoblingen ble brutt',
  failed: 'Tilkobling feilet',
};

/**
 * Utviklingsverktøyet leser oppsettet fra Vite-miljøvariabler slik at ingen
 * serveradresser eller nøkler ligger i kildekoden.
 */
const RENDEZVOUS_URL = import.meta.env.VITE_REMOTE_RENDEZVOUS_URL ?? '';
const RELAY_URL = import.meta.env.VITE_REMOTE_RELAY_URL ?? '';
const SERVER_PUBLIC_KEY = import.meta.env.VITE_REMOTE_SERVER_PUBLIC_KEY ?? '';

export function App() {
  const [deviceId, setDeviceId] = createSignal('');
  const [token, setToken] = createSignal('');
  const [state, setState] = createSignal<DemoState>('idle');
  const [errorMessage, setErrorMessage] = createSignal<string>();
  const [latencyMs, setLatencyMs] = createSignal<number>();
  const [fps, setFps] = createSignal<number>();
  const [displays, setDisplays] = createSignal<readonly RemoteDisplay[]>([]);
  // Tofaktor: feltet vises først når verten faktisk ber om en kode. Vi holder
  // både resolve og reject, slik at et avbrudd faktisk avvisser løftet
  // remote-core venter på i stedet for å forlate det.
  const [secondFactorCode, setSecondFactorCode] = createSignal('');
  const [secondFactorRequest, setSecondFactorRequest] = createSignal<SecondFactorRequest | null>(null);

  let canvasRef: HTMLCanvasElement | undefined;
  let session: RemoteSession | undefined;
  let renderer: CanvasRenderer | undefined;
  let statsTimer: ReturnType<typeof setInterval> | undefined;
  let abortController: AbortController | undefined;
  let cancelledByOperator = false;

  function teardownSession(): void {
    rejectPendingSecondFactor('Tilkoblingen ble avbrutt før koden ble sendt.');
    abortController?.abort();
    abortController = undefined;
    if (statsTimer !== undefined) clearInterval(statsTimer);
    statsTimer = undefined;
    renderer?.detach();
    renderer = undefined;
    session = undefined;
    setDisplays([]);
    setSecondFactorCode('');
  }

  function rejectPendingSecondFactor(message: string): void {
    const pending = secondFactorRequest();
    if (!pending) return;
    setSecondFactorRequest(null);
    pending.reject(new Error(message));
  }

  function requestSecondFactor(): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      setSecondFactorCode('');
      setSecondFactorRequest({ resolve, reject });
    });
  }

  function submitSecondFactor(event: Event): void {
    event.preventDefault();
    const pending = secondFactorRequest();
    const code = secondFactorCode().trim();
    if (!pending || !code) return;
    setSecondFactorRequest(null);
    setSecondFactorCode('');
    pending.resolve(code);
  }

  function cancelConnect(): void {
    cancelledByOperator = true;
    teardownSession();
    setState('idle');
    setErrorMessage('Tilkoblingen ble avbrutt.');
  }

  async function selectDisplay(display: RemoteDisplay): Promise<void> {
    if (!session || display.isPrimary) return;
    const result = await session.actions.execute({ type: 'display.select', actor: 'human', displayId: display.id });
    if (!result.ok) setErrorMessage(result.error?.message ?? 'Kunne ikke bytte skjerm.');
  }

  const isBusy = () => state() === 'connecting' || state() === 'authenticating';
  const isLive = () => state() === 'connected' || state() === 'reconnecting';

  async function handleConnect(): Promise<void> {
    setErrorMessage(undefined);

    if (!deviceId().trim()) {
      setErrorMessage('Enhets-ID er påkrevd.');
      return;
    }
    if (!RENDEZVOUS_URL) {
      setErrorMessage(
        'VITE_REMOTE_RENDEZVOUS_URL er ikke satt. Se README i remote-dev for oppsett mot Support Plane.',
      );
      return;
    }

    setState('connecting');
    cancelledByOperator = false;

    try {
      const client = createRemoteClient({
        rendezvousUrl: RENDEZVOUS_URL,
        relayUrl: RELAY_URL || undefined,
        serverPublicKey: SERVER_PUBLIC_KEY || undefined,
        // Uten serverens offentlige nøkkel kan motparten ikke autentiseres.
        // Vi tillater det KUN her, i utviklingsverktøyet, og aldri som
        // standard — se docs/security.md.
        allowUnverifiedPeer: !SERVER_PUBLIC_KEY,
      });

      abortController = new AbortController();
      const newSession = await client.connect({
        deviceId: deviceId().trim(),
        auth: { token: token(), secondFactor: requestSecondFactor },
        signal: abortController.signal,
      });
      setToken('');
      session = newSession;
      setDisplays(newSession.displays);

      if (canvasRef) {
        renderer = new CanvasRenderer(canvasRef);
        renderer.attach(newSession);
        renderer.resize();
      }

      newSession.on('state', ({ state: nextState }) => {
        setState(nextState);
        // 'reconnecting' beholder renderer og lyttere; kun terminale
        // tilstander river ned.
        if (nextState === 'disconnected' || nextState === 'failed') teardownSession();
      });
      newSession.on('latency', ({ latencyMs: ms }) => setLatencyMs(ms));
      newSession.on('display-change', ({ displays: next }) => setDisplays(next));
      newSession.on('error', (error) => setErrorMessage(error.message));
      newSession.on('disconnect', ({ reason }) => {
        if (newSession.state === 'reconnecting') {
          setErrorMessage(`Forbindelsen ble brutt (${reason}) — kobler til på nytt …`);
          return;
        }
        setErrorMessage(`Frakoblet: ${reason}`);
      });

      statsTimer = setInterval(() => {
        const snapshot = newSession.stats.getSnapshot();
        setFps(snapshot.fps);
        setLatencyMs(snapshot.latencyMs);
      }, 1000);

      setState(newSession.state);
    } catch (error) {
      // Vårt eget avbrudd er ikke en feil; cancelConnect() eier tilstanden.
      if (cancelledByOperator) return;
      teardownSession();
      setState('failed');
      setErrorMessage(error instanceof Error ? error.message : String(error));
    }
  }

  async function handleDisconnect(): Promise<void> {
    try {
      await session?.disconnect();
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : String(error));
    } finally {
      teardownSession();
      setState('idle');
      setLatencyMs(undefined);
      setFps(undefined);
    }
  }

  onCleanup(() => {
    void session?.disconnect();
    teardownSession();
  });

  return (
    <main class="page">
      <h1>remote-core – utviklingsverktøy</h1>
      <p class="disclaimer">
        Dette er kun et internt utviklingsverktøy for <code>@verevon/remote-core</code>. Det er ikke en del av
        Verevon-produktet, og gjenbruker ingen RustDesk-brukergrensesnitt.
      </p>

      <section class="connect-form">
        <label>
          Enhets-ID
          <input
            type="text"
            value={deviceId()}
            onInput={(event) => setDeviceId(event.currentTarget.value)}
            placeholder="123456789"
            disabled={isBusy() || isLive()}
          />
        </label>

        <label>
          Passord/token
          <input
            type="password"
            value={token()}
            onInput={(event) => setToken(event.currentTarget.value)}
            disabled={isBusy() || isLive()}
          />
        </label>

        <Show when={secondFactorRequest() !== null}>
          <form class="second-factor" onSubmit={submitSecondFactor}>
            <label>
              Engangskode (passordet er godkjent, verten krever tofaktor)
              <input
                type="text"
                inputmode="numeric"
                autocomplete="one-time-code"
                value={secondFactorCode()}
                onInput={(event) => setSecondFactorCode(event.currentTarget.value)}
                placeholder="123456"
              />
            </label>
            <button type="submit" disabled={!secondFactorCode().trim()}>
              Send kode
            </button>
          </form>
        </Show>

        <Show
          when={!isLive()}
          fallback={
            <button type="button" onClick={() => void handleDisconnect()}>
              Koble fra
            </button>
          }
        >
          <button type="button" onClick={() => void handleConnect()} disabled={isBusy()}>
            Koble til
          </button>
        </Show>

        <Show when={isBusy() || secondFactorRequest() !== null}>
          <button type="button" onClick={cancelConnect}>
            Avbryt
          </button>
        </Show>
      </section>

      <Show when={displays().length > 1}>
        <section class="displays" aria-label="Velg skjerm">
          <For each={displays()}>
            {(display) => (
              <button
                type="button"
                aria-pressed={display.isPrimary ? 'true' : 'false'}
                disabled={display.isPrimary || state() !== 'connected'}
                onClick={() => void selectDisplay(display)}
              >
                {display.label} ({display.width}×{display.height})
              </button>
            )}
          </For>
        </section>
      </Show>

      <Show when={errorMessage()}>
        <p class="error">{errorMessage()}</p>
      </Show>

      <section class="remote-screen">
        <canvas ref={canvasRef} width={1280} height={720} />
      </section>

      <section class="status-bar">
        <span>Tilstand: {STATE_LABELS[state()]}</span>
        <span>Forsinkelse: {latencyMs() !== undefined ? `${latencyMs()} ms` : '–'}</span>
        <span>Bilder per sekund: {fps() ?? '–'}</span>
      </section>
    </main>
  );
}
