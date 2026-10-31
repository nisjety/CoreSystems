/**
 * Norwegian text for the English strings the product writes into the "Arbeid" rows and the "Teknisk aktivitet" rows:
 * the controller's step texts (src/features/chat/components/use-chat-controller.ts, chat-normalizers.ts and chat-types.ts)
 * and the run's event types, which uiEventLabel names as "run · connected" (src/shared/chat/verevon-ui-events.ts).
 * The product keeps those in English for every user; the demo shows Norwegian only, so exactly these texts are
 * rewritten here, in the demo build, and nowhere else. Each entry is a translation of the same words: nothing is
 * added or invented.
 */
const WORK_ROW_TEXT: ReadonlyMap<string, string> = new Map<string, string>([
  ['Compose response', 'Skriver svar'],
  ['Streaming answer text.', 'Skriver svaret.'],
  ['Waiting for model output.', 'Venter på modellen.'],
  ['Connect stream', 'Kobler til agentstrømmen'],
  ['Connected to the live agent stream.', 'Koblet til agentstrømmen.'],
  ['Regenerating latest response.', 'Lager siste svar på nytt.'],
  ['Model selected', 'Modell valgt'],
  ['Completed.', 'Fullført.'],
  // The screen-reader heading of the Work canvas, which the product leaves in English.
  ['Output', 'Resultat'],
  ['Stopped by the user.', 'Stoppet av brukeren.'],
  ['Stream interrupted', 'Strømmen ble avbrutt.'],
  ['Search', 'Søk'],
  ['Web search is available; it runs only if the answer needs fresh data.', 'Nettsøk er tilgjengelig; det kjøres bare hvis svaret trenger ferske data.'],
  ['No web search needed — answered from existing knowledge or other tools.', 'Ingen nettsøk nødvendig: svaret bygger på eksisterende kunnskap eller andre verktøy.'],
  ['Source found', 'Kilde funnet'],
  ['Answer verification', 'Kontroll av svaret'],
])

/** The run's event types as the "Teknisk aktivitet" rows name them (uiEventLabel: the type with its dot as " · "). */
const ACTIVITY_ROW_TEXT: ReadonlyMap<string, string> = new Map<string, string>([
  ['activity · updated', 'aktivitet · oppdatert'],
  ['approval · decided', 'godkjenning · avgjort'],
  ['approval · requested', 'godkjenning · forespurt'],
  ['artifact · updated', 'dokument · oppdatert'],
  ['attachment · created', 'vedlegg · opprettet'],
  ['citation · added', 'kilde · lagt til'],
  ['delegation · updated', 'delegering · oppdatert'],
  ['followups · suggested', 'oppfølging · foreslått'],
  ['grounding · updated', 'forankring · oppdatert'],
  ['input · queued', 'innspill · satt i kø'],
  ['memory · recalled', 'minne · hentet'],
  ['message · delta', 'melding · tekst'],
  ['message · done', 'melding · ferdig'],
  ['message · started', 'melding · startet'],
  ['reasoning · delta', 'resonnement · tekst'],
  ['receipt · verified', 'kvittering · bekreftet'],
  ['run · cancelled', 'kjøring · avbrutt'],
  ['run · completed', 'kjøring · fullført'],
  ['run · connected', 'kjøring · tilkoblet'],
  ['run · error', 'kjøring · feil'],
  ['run · paused', 'kjøring · satt på pause'],
  ['run · resumed', 'kjøring · gjenopptatt'],
  ['run · stopped', 'kjøring · stoppet'],
  ['state · delta', 'tilstand · endring'],
  ['state · snapshot', 'tilstand · øyeblikksbilde'],
  ['step · updated', 'steg · oppdatert'],
  ['thread · titled', 'samtale · fikk tittel'],
  ['tool · call', 'verktøy · kall'],
  ['tool · result', 'verktøy · resultat'],
  ['usage · recorded', 'forbruk · registrert'],
])

/** The pause kinds of a "run · paused · <kind>" row (VerevonPauseKind in verevon-ui-events.ts). */
const PAUSE_KIND_TEXT: ReadonlyMap<string, string> = new Map<string, string>([
  ['approval', 'godkjenning'],
  ['blocked', 'blokkert'],
  ['ambiguous', 'uklar'],
])

/** Texts with a count or a quoted question: the Norwegian keeps the count or the question as it is. */
const WORK_ROW_PATTERNS: ReadonlyArray<readonly [RegExp, (match: RegExpMatchArray) => string]> = [
  [/^Web search captured (\d+) sources?\.$/, ([, count = '']) => `Nettsøk fant ${count} ${count === '1' ? 'kilde' : 'kilder'}.`],
  [/^Checked against (\d+) web sources? shown in Kilder\.$/, ([, count = '']) => `Kontrollert mot ${count} ${count === '1' ? 'nettkilde' : 'nettkilder'} vist i Kilder.`],
  [/^Preparing "(.+)"\.$/, ([, question = '']) => `Forbereder «${question}».`],
]

/** The Norwegian text for an English product string, or undefined when the text is not one of them. */
export function localiseDemoText(value: string): string | undefined {
  const text = value.trim()
  const known = WORK_ROW_TEXT.get(text) ?? ACTIVITY_ROW_TEXT.get(text)
  if (known !== undefined) return known
  const paused = /^run · paused · (\w+)$/.exec(text)
  if (paused) {
    const kind = PAUSE_KIND_TEXT.get(paused[1] ?? '')
    return kind === undefined ? undefined : `kjøring · satt på pause · ${kind}`
  }
  for (const [pattern, translate] of WORK_ROW_PATTERNS) {
    const match = pattern.exec(text)
    if (match) return translate(match)
  }
  return undefined
}

function localiseNode(node: Text): void {
  const next = localiseDemoText(node.data)
  if (next !== undefined && next !== node.data) node.data = next
}

function localiseTree(root: Node): void {
  if (root.nodeType === Node.TEXT_NODE) {
    localiseNode(root as Text)
    return
  }
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  for (let node = walker.nextNode(); node; node = walker.nextNode()) localiseNode(node as Text)
}

/** Keeps the Arbeid rows Norwegian as the product renders and updates them. Returns a stop function. */
export function startDemoLocalisation(root: Node = document.body): () => void {
  localiseTree(root)
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === 'characterData') localiseTree(record.target)
      else record.addedNodes.forEach((added) => localiseTree(added))
    }
  })
  observer.observe(root, { childList: true, subtree: true, characterData: true })
  return () => observer.disconnect()
}
