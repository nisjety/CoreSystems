import { createSignal, onCleanup, Show } from 'solid-js';
import type { RemoteProtocol, RemoteSession, SessionState } from '@verevon/remote-core';
import { CanvasRenderer, RemoteConnectionError, createRemoteClient } from '@verevon/remote-core';

/**
 * remote-core mangler i denne versjonen en fullstendig
 * RustDesk-protokollimplementasjon å koble til (se docs/architecture.md,
 * "Status", i @verevon/remote-core sin egen pakke). Resten av App-komponenten
 * under er skrevet ferdig og reelt koblet opp (tilstand, statistikk,
 * kanvas-oppkobling, frakobling); dette er det ENE hullet, og det er bevisst
 * en typet feil i stedet for en stille TODO eller en late-som-tilkobling.
 */
function requireProtocol(): RemoteProtocol {
  throw new RemoteConnectionError(
    'remote-core mangler ennå en fullstendig RustDesk-protokollimplementasjon — se docs/architecture.md.',
  );
}

type DemoState = SessionState | 'idle';

const STATE_LABELS: Record<DemoState, string> = {
  idle: 'Ikke tilkoblet',
  connecting: 'Kobler til …',
  authenticating: 'Autentiserer …',
  connected: 'Tilkoblet',
  reconnecting: 'Kobler til på nytt …',
  disconnected: 'Tilkoblingen ble brutt',
  failed: 'Tilkobling feilet',
};

export function App() {
  const [deviceId, setDeviceId] = createSignal('');
  const [token, setToken] = createSignal('');
  const [state, setState] = createSignal<DemoState>('idle');
  const [errorMessage, setErrorMessage] = createSignal<string>();
  const [latencyMs, setLatencyMs] = createSignal<number>();
  const [fps, setFps] = createSignal<number>();

  let canvasRef: HTMLCanvasElement | undefined;
  let session: RemoteSession | undefined;
  let renderer: CanvasRenderer | undefined;
  let statsTimer: ReturnType<typeof setInterval> | undefined;

  function teardownSession(): void {
    if (statsTimer !== undefined) clearInterval(statsTimer);
    statsTimer = undefined;
    renderer?.detach();
    renderer = undefined;
    session = undefined;
  }

  async function handleConnect(): Promise<void> {
    setErrorMessage(undefined);

    if (!deviceId().trim()) {
      setErrorMessage('Enhets-ID er påkrevd.');
      return;
    }

    setState('connecting');

    try {
      const client = createRemoteClient({ protocol: requireProtocol() });
      const newSession = await client.connect({ deviceId: deviceId(), auth: { token: token() } });
      session = newSession;

      if (canvasRef) {
        renderer = new CanvasRenderer(canvasRef);
        renderer.attach(newSession);
      }

      newSession.on('state', ({ state: nextState }) => setState(nextState));
      newSession.on('latency', ({ latencyMs: ms }) => setLatencyMs(ms));
      newSession.on('disconnect', () => teardownSession());

      statsTimer = setInterval(() => {
        const snapshot = newSession.stats.getSnapshot();
        setFps(snapshot.fps);
        setLatencyMs(snapshot.latencyMs);
      }, 1000);

      setState(newSession.state);
    } catch (error) {
      setState('failed');
      setErrorMessage(error instanceof Error ? error.message : String(error));
    }
  }

  async function handleDisconnect(): Promise<void> {
    try {
      await session?.disconnect();
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
            disabled={state() === 'connecting' || state() === 'connected'}
          />
        </label>

        <label>
          Passord/token
          <input
            type="password"
            value={token()}
            onInput={(event) => setToken(event.currentTarget.value)}
            disabled={state() === 'connecting' || state() === 'connected'}
          />
        </label>

        <Show
          when={state() !== 'connected' && state() !== 'connecting'}
          fallback={
            <button type="button" onClick={() => void handleDisconnect()}>
              Koble fra
            </button>
          }
        >
          <button type="button" onClick={() => void handleConnect()} disabled={state() === 'connecting'}>
            Koble til
          </button>
        </Show>
      </section>

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
