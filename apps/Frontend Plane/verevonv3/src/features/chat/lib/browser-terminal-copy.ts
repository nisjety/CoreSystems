// The one place the SPA words a browser run's terminal failure code. The codes are session-core's closed
// vocabulary: the model gateway projects them on the run replay and the BFF relays that unchanged. A code this
// module does not word returns null, so the person never sees raw server text. The module is pure: it stores,
// logs and sends nothing, so a zero-retention run needs no skip.

/**
 * The reason a browser run ended, in the reader's language, or null for a code this module does not word.
 * `tr` is the caller's translator, so Norwegian and English switch together.
 */
export function browserTerminalFailureCopy(code: unknown, tr: (no: string, en: string) => string): string | null {
  // Held as an object so each sentence is a `.tr(no, en)` call, the form the UI copy gate accepts in a new file
  // (see limit-notices.ts).
  const copy = { tr }
  switch (code) {
    case 'browser_continue_tab_changed':
      return copy.tr('Fanen er endret. Velg fanen på nytt for å fortsette.', 'The tab has changed. Select it again to continue.')
    case 'browser_continue_origin_changed':
      return copy.tr('Fanen åpnet et annet nettsted. Velg fanen på nytt hvis Verevon skal fortsette der.', 'The tab opened another site. Select the tab again to let Verevon continue there.')
    case 'browser_permission_denied':
    case 'browser_agent_permission_denied':
      return copy.tr('Nettleseragenten mangler tilgang. Kontroller agentens tilgang og tjenestestatus før du prøver igjen.', 'The browser agent lacks permission. Check its access and service status before trying again.')
    case 'browser_timed_out':
    case 'browser_agent_timed_out':
      return copy.tr('Nettleseroppgaven tok for lang tid. Prøv en kortere oppgave.', 'The browser task timed out. Try a shorter task.')
    case 'browser_resource_exhausted':
    case 'browser_agent_resource_exhausted':
      return copy.tr('Nettleseragenten nådde en ressursgrense. Prøv igjen senere eller bruk en kortere oppgave.', 'The browser agent reached a resource limit. Try again later or use a shorter task.')
    // A takeover whose hand-back timed out ends the run; the person is told how to start it again.
    case 'browser_help_timed_out':
      return copy.tr('Ingen tok over nettleseren i tide, så kjøringen ble avsluttet. Start oppgaven på nytt når du er klar til å hjelpe.', 'Nobody took over the browser in time, so the run ended. Start the task again when you are ready to help.')
    default:
      return null
  }
}
