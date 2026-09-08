import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { Eye, Keyboard, MonitorPlay, MousePointer2, Pause, Play, ShieldCheck } from '@/shared/icons'
import { getRemoteSupportConfig, type RemoteSupportConfig } from '@/shared/api/remote-support-client'
import {
  CanvasRenderer,
  createRemoteClient,
  releaseVideoFrame,
  type AIObserver,
  type PointerButton,
  type RemoteDisplay,
  type RemotePermission,
  type QualityLevel,
  type RemoteSession,
  type SessionState,
} from '@verevon/remote-core'

type PageState = SessionState | 'idle'

interface SecondFactorRequest {
  readonly resolve: (code: string) => void
  readonly reject: (error: Error) => void
}

/** Nettleserens `MouseEvent.button` → remote-core sin knappmodell. */
function pointerButton(button: number): PointerButton | null {
  if (button === 0) return 'left'
  if (button === 1) return 'middle'
  if (button === 2) return 'right'
  return null
}

const PERMISSION_LABELS: Record<RemotePermission, [string, string]> = {
  'screen.view': ['Se skjerm', 'View screen'],
  'input.pointer': ['Styre mus', 'Control mouse'],
  'input.keyboard': ['Styre tastatur', 'Control keyboard'],
  'clipboard.read': ['Lese utklippstavle', 'Read clipboard'],
  'clipboard.write': ['Skrive utklippstavle', 'Write clipboard'],
  'files.read': ['Lese filer', 'Read files'],
  'files.write': ['Skrive filer', 'Write files'],
  'audio.listen': ['Lytte til lyd', 'Listen to audio'],
}

/**
 * 'auto' først: det er standarden, og betyr at vertens egen adaptive
 * bitrate-løkke styrer. De tre andre pinner et nivå hos verten.
 */
const QUALITY_LEVELS: readonly QualityLevel[] = ['auto', 'low', 'medium', 'high']

const QUALITY_LABELS: Record<QualityLevel, [string, string]> = {
  auto: ['Automatisk', 'Automatic'],
  low: ['Lav', 'Low'],
  medium: ['Balansert', 'Balanced'],
  high: ['Best', 'Best'],
}

/** Rekkefølgen vi viser tillatelser i — de som gjelder MVP først. */
const SHOWN_PERMISSIONS: readonly RemotePermission[] = [
  'screen.view',
  'input.pointer',
  'input.keyboard',
  'clipboard.write',
]

/**
 * Live fjernhjelp-økt: agenten ser og — når kunden har gitt tillatelse —
 * styrer kundens skjerm gjennom `@verevon/remote-core`. Selve økten går
 * nettleser→Support Plane over WSS og passerer aldri gateway-en; gateway-en
 * leverer bare tilkoblingsoppsettet.
 *
 * Det som bevisst IKKE finnes her ennå, og som siden sier fra om i stedet for
 * å late som: (1) en varig øktlogg — ingen kjerne eier den enda, (2) å sende
 * AI-observerte bilder til Verevon AI — det trenger en Model Plane-kontrakt
 * med ZDR-håndtering. Observatøren kjører reelt og teller bilder lokalt.
 */
export default function RemoteSupportPage() {
  const i18n = useI18n()

  const [config, { refetch: refetchConfig }] = createResource<RemoteSupportConfig>(() => getRemoteSupportConfig())

  const [deviceId, setDeviceId] = createSignal('')
  const [token, setToken] = createSignal('')
  const [state, setState] = createSignal<PageState>('idle')
  const [errorMessage, setErrorMessage] = createSignal<string>()
  const [permissions, setPermissions] = createSignal<readonly RemotePermission[]>([])
  const [latencyMs, setLatencyMs] = createSignal<number>()
  const [fps, setFps] = createSignal<number>()
  const [displays, setDisplays] = createSignal<readonly RemoteDisplay[]>([])
  const [aiPaused, setAiPaused] = createSignal(true)
  const [aiFrames, setAiFrames] = createSignal(0)
  const [bitrateKbps, setBitrateKbps] = createSignal<number>()
  // Vertens eget måltall (kbps). Skilt fra vår egen måling over — det ene er
  // hva verten SIKTER mot, det andre hva vi faktisk mottar.
  const [targetBitrateKbps, setTargetBitrateKbps] = createSignal<number>()
  const [quality, setQuality] = createSignal<QualityLevel>('auto')
  // Tofaktor: verten ber om en kode ETTER godkjent passord. Vi viser feltet
  // først da, og løser løftet remote-core venter på når agenten sender inn.
  //
  // Vi holder BÅDE resolve og reject: forlater agenten siden mens koden
  // etterspørres, må løftet avvises. Slapp vi bare taket i det, ville
  // remote-core ventet for alltid på en kode som aldri kommer — og reléet
  // aldri blitt lukket.
  const [secondFactorCode, setSecondFactorCode] = createSignal('')
  const [secondFactorRequest, setSecondFactorRequest] = createSignal<SecondFactorRequest | null>(null)

  let canvasRef: HTMLCanvasElement | undefined
  let session: RemoteSession | undefined
  let renderer: CanvasRenderer | undefined
  let observer: AIObserver | undefined
  let resizeObserver: ResizeObserver | undefined
  let statsTimer: ReturnType<typeof setInterval> | undefined
  let pendingMove: { x: number; y: number } | undefined
  let moveFrame: number | undefined
  // Den ekte avbryte-mekanismen: remote-core følger denne gjennom hele
  // tilkoblingssekvensen og rydder opp reléet når den utløses.
  let abortController: AbortController | undefined
  // Settes når VI avbrøt. Avbruddet får connect() til å avvise, og uten dette
  // flagget ville catch-grenen overskrevet «avbrutt» med «tilkobling feilet».
  let cancelledByOperator = false

  const isBusy = () => state() === 'connecting' || state() === 'authenticating'
  const isLive = () => state() === 'connected' || state() === 'reconnecting'
  const awaitingSecondFactor = () => secondFactorRequest() !== null
  const canCancel = () => isBusy() || awaitingSecondFactor()
  const activeDisplay = createMemo(() => displays().find((display) => display.isPrimary) ?? displays()[0])
  const canPoint = createMemo(() => permissions().includes('input.pointer'))
  const canType = createMemo(() => permissions().includes('input.keyboard'))

  const stateLabel = createMemo(() => {
    const labels: Record<PageState, [string, string]> = {
      idle: ['Ikke tilkoblet', 'Not connected'],
      connecting: ['Kobler til …', 'Connecting…'],
      authenticating: ['Autentiserer …', 'Authenticating…'],
      connected: ['Tilkoblet', 'Connected'],
      reconnecting: ['Kobler til på nytt …', 'Reconnecting…'],
      disconnected: ['Tilkoblingen ble brutt', 'Disconnected'],
      failed: ['Tilkobling feilet', 'Connection failed'],
    }
    return i18n.tr(...labels[state()])
  })

  function teardown(): void {
    // Rekkefølge: avvis den ventende koden FØR vi nuller signalet, ellers
    // sitter remote-core igjen med et løfte ingen eier.
    rejectPendingSecondFactor('Tilkoblingen ble avbrutt før koden ble sendt.')
    abortController?.abort()
    abortController = undefined
    if (statsTimer !== undefined) clearInterval(statsTimer)
    statsTimer = undefined
    if (moveFrame !== undefined) cancelAnimationFrame(moveFrame)
    moveFrame = undefined
    pendingMove = undefined
    resizeObserver?.disconnect()
    resizeObserver = undefined
    observer?.close()
    observer = undefined
    renderer?.detach()
    renderer = undefined
    session = undefined
    setPermissions([])
    setDisplays([])
    setAiFrames(0)
    setAiPaused(true)
    setSecondFactorCode('')
    setBitrateKbps(undefined)
    setTargetBitrateKbps(undefined)
    setQuality('auto')
  }

  async function selectQuality(level: QualityLevel): Promise<void> {
    if (!session) return
    const previous = quality()
    setQuality(level)
    const result = await session.actions.execute({ type: 'quality.set', actor: 'human', level })
    if (!result.ok) {
      setQuality(previous)
      setErrorMessage(result.error?.message ?? i18n.tr('Kunne ikke endre bildekvalitet.', 'Could not change image quality.'))
    }
  }

  function rejectPendingSecondFactor(message: string): void {
    const pending = secondFactorRequest()
    if (!pending) return
    setSecondFactorRequest(null)
    pending.reject(new Error(message))
  }

  /** Kalles av remote-core når verten krever en andre faktor; løses fra skjemaet under. */
  function requestSecondFactor(): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      setSecondFactorCode('')
      setSecondFactorRequest({ resolve, reject })
    })
  }

  function submitSecondFactor(event: Event): void {
    event.preventDefault()
    const pending = secondFactorRequest()
    const code = secondFactorCode().trim()
    if (!pending || !code) return
    setSecondFactorRequest(null)
    // Koden er brukt opp i samme øyeblikk — la den ikke ligge igjen i minnet.
    setSecondFactorCode('')
    pending.resolve(code)
  }

  /** Avbryter et pågående tilkoblingsforsøk (også mens vi venter på en kode). */
  function cancelConnect(): void {
    cancelledByOperator = true
    teardown()
    setState('idle')
    setErrorMessage(i18n.tr('Tilkoblingen ble avbrutt.', 'The connection attempt was cancelled.'))
  }

  async function selectDisplay(display: RemoteDisplay): Promise<void> {
    if (!session || display.isPrimary) return
    const result = await session.actions.execute({ type: 'display.select', actor: 'human', displayId: display.id })
    if (!result.ok) setErrorMessage(result.error?.message ?? i18n.tr('Kunne ikke bytte skjerm.', 'Could not switch display.'))
  }

  onCleanup(() => {
    void session?.disconnect()
    teardown()
  })

  async function connect(): Promise<void> {
    setErrorMessage(undefined)
    const current = config()
    if (!current?.configured || !current.rendezvousUrl || !current.serverPublicKey) {
      setErrorMessage(i18n.tr('Fjernhjelp er ikke satt opp på denne serveren.', 'Remote support is not configured on this server.'))
      return
    }
    if (!deviceId().trim()) {
      setErrorMessage(i18n.tr('Enhets-ID er påkrevd.', 'Device ID is required.'))
      return
    }
    if (!canvasRef) return

    setState('connecting')
    cancelledByOperator = false
    try {
      // Nøkkelen kommer alltid fra gateway-en, så motparten kan autentiseres —
      // produktflaten tillater aldri et uverifisert håndtrykk.
      const client = createRemoteClient({
        rendezvousUrl: current.rendezvousUrl,
        relayUrl: current.relayUrl ?? undefined,
        serverPublicKey: current.serverPublicKey,
      })

      abortController = new AbortController()
      const next = await client.connect({
        deviceId: deviceId().trim(),
        auth: { token: token(), secondFactor: requestSecondFactor },
        signal: abortController.signal,
      })
      // Passordet er levert til klienten (som holder sin egen autentikator).
      // docs/security.md sier det bare skal finnes i minnet så lenge
      // tilkoblingsforsøket varer — så vi tømmer feltet her, ikke ved
      // frakobling.
      setToken('')
      session = next
      setPermissions(next.permissions.list())
      setDisplays(next.displays)

      renderer = new CanvasRenderer(canvasRef)
      renderer.attach(next)
      renderer.resize()
      resizeObserver = new ResizeObserver(() => renderer?.resize())
      resizeObserver.observe(canvasRef)

      next.on('state', ({ state: nextState }) => {
        setState(nextState)
        // Kun terminale tilstander river ned visningen. 'reconnecting' beholder
        // renderer og lyttere — økten kommer tilbake på samme objekt.
        if (nextState === 'disconnected' || nextState === 'failed') teardown()
      })
      next.on('permission-change', ({ permissions: granted }) => setPermissions(granted))
      next.on('latency', ({ latencyMs: ms }) => setLatencyMs(ms))
      next.on('quality', ({ level, targetBitrateKbps: target }) => {
        setQuality(level)
        if (target !== undefined) setTargetBitrateKbps(target)
      })
      next.on('display-change', ({ displays: next }) => setDisplays(next))
      next.on('error', (error) => setErrorMessage(error.message))
      next.on('disconnect', ({ reason }) => {
        if (next.state === 'reconnecting') {
          setErrorMessage(
            i18n.tr(`Forbindelsen ble brutt (${reason}) — kobler til på nytt …`, `Connection lost (${reason}) — reconnecting…`),
          )
          return
        }
        setErrorMessage(i18n.tr(`Frakoblet (${reason}).`, `Disconnected (${reason}).`))
      })

      // Observatøren er reell og begrenset til 0,5 bilder/s, men starter
      // pauset: ingen skjermbilder samles før agenten slår det på eksplisitt.
      observer = next.ai.createObserver({ maxFramesPerSecond: 0.5 })
      observer.pause()
      observer.on('frame', (frame) => {
        setAiFrames((count) => count + 1)
        // Vi eier kopien observatøren gir oss — se media/VideoFrame.ts.
        releaseVideoFrame(frame)
      })

      statsTimer = setInterval(() => {
        if (!session) return
        const snapshot = session.stats.getSnapshot()
        setFps(snapshot.fps)
        setLatencyMs(snapshot.latencyMs)
        setBitrateKbps(snapshot.bitrateKbps)
      }, 1000)

      setState(next.state)
      canvasRef.focus()
    } catch (error) {
      // Et avbrudd vi selv utløste er ikke en feil — cancelConnect() har
      // allerede satt riktig tilstand og melding.
      if (cancelledByOperator) return
      teardown()
      setState('failed')
      setErrorMessage(error instanceof Error ? error.message : String(error))
    }
  }

  async function disconnect(): Promise<void> {
    try {
      await session?.disconnect()
    } finally {
      teardown()
      setState('idle')
      setLatencyMs(undefined)
      setFps(undefined)
    }
  }

  function toggleAi(): void {
    if (!observer) return
    if (observer.isPaused) {
      observer.resume()
      setAiPaused(false)
    } else {
      observer.pause()
      setAiPaused(true)
    }
  }

  // Musebevegelser samles per animasjonsramme: en rå `pointermove`-strøm kan
  // gi hundrevis av meldinger i sekundet, langt mer enn verten kan bruke.
  function queueMove(clientX: number, clientY: number): void {
    if (!renderer || !canPoint()) return
    pendingMove = renderer.clientToRemote(clientX, clientY)
    if (moveFrame !== undefined) return
    moveFrame = requestAnimationFrame(() => {
      moveFrame = undefined
      const target = pendingMove
      pendingMove = undefined
      if (target && session) void session.pointer.move(target)
    })
  }

  function onPointerDown(event: PointerEvent): void {
    if (!renderer || !session || !canPoint()) return
    const button = pointerButton(event.button)
    if (!button) return
    event.preventDefault()
    canvasRef?.focus()
    void session.pointer.down({ button, ...renderer.clientToRemote(event.clientX, event.clientY) })
  }

  function onPointerUp(event: PointerEvent): void {
    if (!renderer || !session || !canPoint()) return
    const button = pointerButton(event.button)
    if (!button) return
    event.preventDefault()
    void session.pointer.up({ button, ...renderer.clientToRemote(event.clientX, event.clientY) })
  }

  function onWheel(event: WheelEvent): void {
    if (!renderer || !session || !canPoint()) return
    event.preventDefault()
    void session.pointer.scroll({ deltaX: event.deltaX, deltaY: event.deltaY, ...renderer.clientToRemote(event.clientX, event.clientY) })
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (!session || !canType()) return
    event.preventDefault()
    void session.keyboard.keyDown(event.key)
  }

  function onKeyUp(event: KeyboardEvent): void {
    if (!session || !canType()) return
    event.preventDefault()
    void session.keyboard.keyUp(event.key)
  }

  return (
    <div class="verevon-remote-support" aria-label={i18n.tr('Fjernhjelp', 'Remote support')}>
      <aside class="verevon-remote-support__rail">
        <header>
          <MonitorPlay />
          <span>{i18n.tr('Fjernhjelp', 'Remote support')}</span>
        </header>

        <Show when={config.loading}>
          <p class="verevon-remote-support__notice">{i18n.tr('Henter oppsett …', 'Loading configuration…')}</p>
        </Show>

        <Show when={config.error}>
          <p class="verevon-remote-support__notice verevon-remote-support__notice--error" role="alert">
            {i18n.tr('Kunne ikke hente oppsettet for fjernhjelp.', 'Could not load the remote-support configuration.')}{' '}
            <button type="button" onClick={() => void refetchConfig()}>{i18n.tr('Prøv igjen', 'Retry')}</button>
          </p>
        </Show>

        <Show when={config() && !config()!.configured}>
          <p class="verevon-remote-support__notice" role="status">
            {i18n.tr(
              'Fjernhjelp er ikke satt opp på denne serveren ennå. Mangler:',
              'Remote support is not configured on this server yet. Missing:',
            )}{' '}
            <For each={config()!.missing}>{(name) => <><code>{name}</code>{' '}</>}</For>
          </p>
        </Show>

        <label class="verevon-remote-support__field">
          {i18n.tr('Enhets-ID', 'Device ID')}
          <input
            type="text"
            inputmode="numeric"
            autocomplete="off"
            value={deviceId()}
            onInput={(event) => setDeviceId(event.currentTarget.value)}
            placeholder="123 456 789"
            disabled={isBusy() || isLive()}
          />
        </label>

        <label class="verevon-remote-support__field">
          {i18n.tr('Passord fra kunden', 'Password from the customer')}
          <input
            type="password"
            autocomplete="off"
            value={token()}
            onInput={(event) => setToken(event.currentTarget.value)}
            disabled={isBusy() || isLive()}
          />
        </label>

        <Show when={awaitingSecondFactor()}>
          <form class="verevon-remote-support__field" onSubmit={submitSecondFactor}>
            {i18n.tr('Engangskode fra kundens maskin', "One-time code from the customer's machine")}
            <input
              type="text"
              inputmode="numeric"
              autocomplete="one-time-code"
              value={secondFactorCode()}
              onInput={(event) => setSecondFactorCode(event.currentTarget.value)}
              placeholder="123 456"
              aria-describedby="verevon-remote-support-2fa-hint"
              ref={(element) => queueMicrotask(() => element.focus())}
            />
            <small id="verevon-remote-support-2fa-hint">
              {i18n.tr(
                'Passordet er godkjent. Kundens maskin krever i tillegg en engangskode fra sin autentiseringsapp.',
                "The password was accepted. The customer's machine additionally requires a one-time code from their authenticator app.",
              )}
            </small>
            <Button type="submit" variant="primary" disabled={!secondFactorCode().trim()}>
              {i18n.tr('Send kode', 'Submit code')}
            </Button>
          </form>
        </Show>

        <div class="verevon-remote-support__actions">
          <Show
            when={!isLive()}
            fallback={
              <Button type="button" variant="secondary" onClick={() => void disconnect()}>
                {i18n.tr('Koble fra', 'Disconnect')}
              </Button>
            }
          >
            <Button type="button" variant="primary" disabled={isBusy() || !config()?.configured} onClick={() => void connect()}>
              {isBusy() ? i18n.tr('Kobler til …', 'Connecting…') : i18n.tr('Koble til', 'Connect')}
            </Button>
          </Show>
          {/* Uten denne var det ingen vei ut mens vi ventet på en kode: 'Koble
              til' er avslått og 'Koble fra' vises først når økten er live. */}
          <Show when={canCancel()}>
            <Button type="button" variant="ghost" onClick={cancelConnect}>
              {i18n.tr('Avbryt', 'Cancel')}
            </Button>
          </Show>
        </div>

        <Show when={errorMessage()}>
          <p class="verevon-remote-support__notice verevon-remote-support__notice--error" role="alert">{errorMessage()}</p>
        </Show>

        <p class="verevon-remote-support__notice">
          <ShieldCheck style={{ width: '12px', height: '12px', 'vertical-align': '-2px' }} />{' '}
          {i18n.tr(
            'Kunden må godta tilkoblingen på sin maskin. Styring krever egen tillatelse fra kunden og gis aldri automatisk.',
            'The customer must accept the connection on their machine. Control requires a separate permission from the customer and is never granted automatically.',
          )}
        </p>

        <Show when={isLive()}>
          <dl class="verevon-remote-support__status">
            <div><dt>{i18n.tr('Tilstand', 'State')}</dt><dd>{stateLabel()}</dd></div>
            <div><dt>{i18n.tr('Skjerm', 'Display')}</dt><dd>{activeDisplay()?.label ?? '–'}</dd></div>
            <div><dt>{i18n.tr('Forsinkelse', 'Latency')}</dt><dd>{latencyMs() ? `${latencyMs()} ms` : '–'}</dd></div>
            <div><dt>{i18n.tr('Bilder/s', 'Frames/s')}</dt><dd>{fps() ?? '–'}</dd></div>
            <div>
              <dt>{i18n.tr('Datarate', 'Bitrate')}</dt>
              <dd>{bitrateKbps() !== undefined ? `${bitrateKbps()} kbit/s` : '–'}</dd>
            </div>
          </dl>

          <div class="verevon-remote-support__quality" role="group" aria-label={i18n.tr('Bildekvalitet', 'Image quality')}>
            <span class="verevon-remote-support__quality-label">{i18n.tr('Bildekvalitet', 'Image quality')}</span>
            <For each={QUALITY_LEVELS}>
              {(level) => (
                <button
                  type="button"
                  class={{
                    'verevon-remote-support__quality-option': true,
                    'verevon-remote-support__quality-option--active': quality() === level,
                  }}
                  aria-pressed={quality() === level ? 'true' : 'false'}
                  disabled={state() !== 'connected'}
                  onClick={() => void selectQuality(level)}
                >
                  {i18n.tr(...QUALITY_LABELS[level])}
                </button>
              )}
            </For>
            <Show when={targetBitrateKbps() !== undefined}>
              <small>
                {i18n.tr(
                  `Verten sikter mot ${targetBitrateKbps()} kbit/s`,
                  `Host is targeting ${targetBitrateKbps()} kbit/s`,
                )}
              </small>
            </Show>
          </div>

          <Show when={displays().length > 1}>
            <div class="verevon-remote-support__displays" role="group" aria-label={i18n.tr('Velg skjerm', 'Choose display')}>
              <For each={displays()}>
                {(display) => (
                  <button
                    type="button"
                    class={{ 'verevon-remote-support__display': true, 'verevon-remote-support__display--active': display.isPrimary }}
                    aria-pressed={display.isPrimary ? 'true' : 'false'}
                    disabled={display.isPrimary || state() !== 'connected'}
                    onClick={() => void selectDisplay(display)}
                  >
                    <MonitorPlay />
                    <span>{display.label}</span>
                    <small>{display.width}×{display.height}</small>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <div class="verevon-remote-support__permissions" aria-label={i18n.tr('Tillatelser fra kunden', 'Permissions from the customer')}>
            <For each={SHOWN_PERMISSIONS}>
              {(permission) => (
                <span class={{ 'verevon-remote-support__permission': true, 'verevon-remote-support__permission--granted': permissions().includes(permission) }}>
                  <Show when={permission === 'input.pointer'} fallback={<Show when={permission === 'input.keyboard'} fallback={<Eye />}><Keyboard /></Show>}><MousePointer2 /></Show>
                  {i18n.tr(...PERMISSION_LABELS[permission])}
                </span>
              )}
            </For>
          </div>

          <div class="verevon-remote-support__notice">
            <strong>{i18n.tr('Verevon AI', 'Verevon AI')}</strong>
            {' — '}
            {aiPaused()
              ? i18n.tr('observerer ikke.', 'not observing.')
              : i18n.tr(`observerer (${aiFrames()} bilder samlet).`, `observing (${aiFrames()} frames sampled).`)}
            {' '}
            <Button type="button" variant="ghost" size="xs" onClick={toggleAi}>
              <Show when={aiPaused()} fallback={<><Pause /> {i18n.tr('Pause', 'Pause')}</>}><Play /> {i18n.tr('Start', 'Start')}</Show>
            </Button>
            <br />
            <small>
              {i18n.tr(
                'Bildene samles lokalt i nettleseren og sendes ikke videre ennå — tilkoblingen til Verevon AI kommer med en egen Model Plane-kontrakt.',
                'Frames are sampled locally in the browser and not forwarded yet — the Verevon AI link ships with its own Model Plane contract.',
              )}
            </small>
          </div>
        </Show>
      </aside>

      <section class="verevon-remote-support__stage">
        <div class="verevon-remote-support__stage-bar">
          <span>
            <strong>{stateLabel()}</strong>
            <Show when={isLive() && !canPoint()}> · {i18n.tr('Kun visning — kunden har ikke gitt styring.', 'View only — the customer has not granted control.')}</Show>
          </span>
          <Show when={isLive()}>
            <span>{deviceId()}</span>
          </Show>
        </div>
        <div class="verevon-remote-support__screen">
          <canvas
            ref={canvasRef}
            width={1280}
            height={720}
            tabindex="0"
            data-view-only={isLive() && !canPoint() ? 'true' : 'false'}
            aria-label={i18n.tr('Kundens skjerm', "The customer's screen")}
            onPointerMove={(event) => queueMove(event.clientX, event.clientY)}
            onPointerDown={onPointerDown}
            onPointerUp={onPointerUp}
            onWheel={onWheel}
            onKeyDown={onKeyDown}
            onKeyUp={onKeyUp}
            onContextMenu={(event) => event.preventDefault()}
          />
          <Show when={!isLive()}>
            <div class="verevon-remote-support__screen-empty">
              <MonitorPlay />
              <span>
                {i18n.tr(
                  'Skriv inn enhets-ID og passordet kunden leser opp, og koble til. Kundens skjerm vises her.',
                  "Enter the device ID and the password the customer reads out, then connect. The customer's screen appears here.",
                )}
              </span>
            </div>
          </Show>
        </div>
      </section>
    </div>
  )
}
