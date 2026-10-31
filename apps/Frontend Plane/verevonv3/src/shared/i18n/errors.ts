// Norwegian-first mapping from gateway/auth error `code`s to user-facing copy.
//
// The gateway (and the Better Auth service it proxies) return a structured
// `{ code, message }` pair on every error response, but `message` is always
// English ops/dev text — never meant for a Norwegian end user to read verbatim.
// Callers should translate by `code` via `translateApiError`, not display
// `err.message` directly. Unknown/unmapped codes fall back to a safe, generic
// Norwegian message instead of leaking the raw English string.
//
// Convention: a code the gateway can emit and a person can hit gets an entry here.
// `browser-errors.test.ts` scans every gateway domain file and fails on an emitted
// code that has neither an entry nor a reasoned internal-only exemption, so the
// table cannot drift. A code answered by status class (401, 403, 429, no network)
// needs no entry; everything else falls back to the caller's own sentence.

type ErrorCopy = {
  no: string
  en: string
  /** A "try again in N seconds" sentence (from the error's retry hint) is appended to the text. */
  retryable?: boolean
}

type ApiErrorLike =
  | {
      code?: string | null
      status?: number
      retryAfterSeconds?: number | null
      blockReason?: string | null
      resumable?: boolean
    }
  | null
  | undefined

const GENERIC_FALLBACK: ErrorCopy = {
  no: 'Noe gikk galt. Prøv igjen, eller kontakt support hvis problemet vedvarer.',
  en: 'Something went wrong. Try again, or contact support if the problem persists.',
}

/**
 * An error the SPA raises itself, with a sentence already written for the person
 * in both languages ("A project with this name already exists", "Create the
 * organization first"). {@link translateApiError} and {@link describeApiError}
 * show this copy as it is, because a catch site that only knows its own
 * "could not do X" fallback would otherwise hide the reason the person can act
 * on. An error that is not one of these (or an ApiError) is never shown raw.
 */
export class LocalizedError extends Error {
  readonly copy: { no: string; en: string }

  constructor(copy: { no: string; en: string }) {
    super(copy.en)
    this.name = 'LocalizedError'
    this.copy = copy
  }
}

export const ERROR_COPY: Record<string, ErrorCopy> = {
  // Cross-cutting / structural
  validation_error: { no: 'Noen av feltene er ikke gyldige. Sjekk og prøv igjen.', en: 'Some fields are invalid. Check and try again.' },
  invalid_input: { no: 'Forespørselen inneholder ugyldige data.', en: 'The request contains invalid data.' },
  invalid_json: { no: 'Forespørselen kunne ikke leses.', en: 'The request could not be read.' },
  unauthenticated: { no: 'Du må logge inn på nytt.', en: 'You need to sign in again.' },
  forbidden: { no: 'Du har ikke tilgang til å gjøre dette.', en: 'You do not have permission to do this.' },
  not_implemented: { no: 'Denne funksjonen er ikke tilgjengelig ennå.', en: 'This feature is not available yet.' },
  confirmation_required: { no: 'Denne handlingen må bekreftes først.', en: 'This action needs to be confirmed first.' },
  create_failed: { no: 'Kunne ikke opprette dette. Prøv igjen.', en: 'Could not create this. Try again.' },
  upstream_error: { no: 'En bakenforliggende tjeneste svarte med en feil. Prøv igjen.', en: 'A backing service returned an error. Try again.' },
  upstream_unavailable: { no: 'Tjenesten er utilgjengelig akkurat nå. Prøv igjen om litt.', en: 'The service is unavailable right now. Try again shortly.' },
  upstream_shape_error: { no: 'Fikk et uventet svar fra tjenesten. Prøv igjen.', en: 'Got an unexpected response from the service. Try again.' },
  upload_requires_multipart: { no: 'Opplasting krever filvedlegg, ikke bare tekst.', en: 'Upload requires a file attachment, not just text.' },

  // Organization / membership (onboarding-adjacent)
  no_active_org: { no: 'Velg en organisasjon først.', en: 'Select an organization first.' },
  org_scope_required: { no: 'Denne handlingen krever en valgt organisasjon.', en: 'This action requires an organization to be selected.' },
  organization_required: { no: 'En organisasjon må være valgt for å fortsette.', en: 'An organization must be selected to continue.' },
  organization_access_denied: { no: 'Du har ikke tilgang til denne organisasjonen.', en: 'You do not have access to this organization.' },
  organization_conflict: { no: 'Organisasjonen ble endret av noen andre. Last inn på nytt og prøv igjen.', en: 'The organization was changed by someone else. Reload and try again.' },
  organization_lookup_failed: { no: 'Fant ikke organisasjonen. Prøv igjen.', en: 'Could not find the organization. Try again.' },
  organization_not_ready: { no: 'Organisasjonen er ikke klar ennå. Prøv igjen om litt.', en: 'The organization is not ready yet. Try again shortly.' },
  organization_owner_required: { no: 'Kun organisasjonens eier kan gjøre dette.', en: 'Only the organization owner can do this.' },
  organization_provisioning_failed: { no: 'Kunne ikke sette opp organisasjonen. Prøv igjen.', en: 'Could not provision the organization. Try again.' },
  organization_recovery_required: { no: 'Organisasjonen må gjenopprettes før du kan fortsette. Kontakt support.', en: 'The organization needs to be recovered before you can continue. Contact support.' },
  organization_activation_failed: { no: 'Kunne ikke aktivere organisasjonen. Prøv igjen.', en: 'Could not activate the organization. Try again.' },
  organization_membership_required: { no: 'Du må være medlem av denne organisasjonen.', en: 'You need to be a member of this organization.' },
  organization_membership_stale: { no: 'Medlemskapet ditt er ikke oppdatert. Last inn siden på nytt.', en: 'Your membership is out of date. Reload the page.' },
  organization_membership_unavailable: { no: 'Kunne ikke bekrefte medlemskapet ditt akkurat nå.', en: 'Could not verify your membership right now.' },
  organization_membership_authority_unavailable: { no: 'Kunne ikke bekrefte medlemskapet ditt akkurat nå.', en: 'Could not verify your membership right now.' },
  membership_lookup_failed: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  membership_lookup_invalid: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  membership_response_invalid: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  billing_admin_required: { no: 'Kun en administrator kan gjøre dette.', en: 'Only an admin can do this.' },

  // Inbox / tickets / conversations
  thread_id_required: { no: 'Ingen samtale er valgt.', en: 'No conversation is selected.' },
  conversation_membership_required: { no: 'Du har ikke tilgang til denne samtalen.', en: 'You do not have access to this conversation.' },
  conversation_upstream_target_rejected: { no: 'Meldingen kunne ikke sendes til mottakeren.', en: 'The message could not be delivered to the recipient.' },
  // A channel send is not a normal retry: the provider may have received it
  // even though we did not get a final response. Preserve that operational
  // distinction so an agent cannot accidentally double-send to a customer.
  delivery_unknown: { no: 'Vi kan ikke bekrefte om svaret ble sendt. Ikke send på nytt automatisk; avstem først.', en: 'We cannot confirm whether the reply was sent. Do not retry automatically; reconcile first.' },
  send_failed: { no: 'Kundekanalen godtok ikke svaret. Det ble ikke sendt.', en: 'The customer channel did not accept the reply. It was not sent.' },
  delivery_unavailable: { no: 'Kundekanalen er ikke satt opp for utsending. Svaret ble ikke sendt eller lagret.', en: 'The customer channel is not configured for sending. The reply was not sent or stored.' },
  support_intake_unavailable: { no: 'Support-tjenesten er utilgjengelig akkurat nå.', en: 'The support service is unavailable right now.' },
  auth_callback_unavailable: { no: 'Tilkoblingen er utilgjengelig akkurat nå. Prøv igjen.', en: 'The connection is unavailable right now. Try again.' },
  auth_callback_response_invalid: { no: 'Fikk et ugyldig svar under tilkobling. Prøv igjen.', en: 'Got an invalid response while connecting. Try again.' },
  auth_callback_response_too_large: { no: 'Svaret under tilkobling var for stort. Prøv igjen.', en: 'The response while connecting was too large. Try again.' },
  auth_callback_target_rejected: { no: 'Tilkoblingen ble avvist.', en: 'The connection was rejected.' },
  notification_upstream_target_rejected: { no: 'Varselet kunne ikke leveres.', en: 'The notification could not be delivered.' },

  // Knowledge base / crawl / documents
  crawl_backlog_full: { no: 'Det venter for mange gjennomganger på organisasjonen din. Prøv igjen om litt.', en: 'Too many crawls are waiting for your organization. Try again shortly.' },
  crawl_allowance_exhausted: { no: 'Organisasjonen din har brukt opp kvoten for gjennomgang av nettsteder. Kontakt en administrator eller prøv igjen når kvoten fornyes.', en: 'Your organization has used up its crawl allowance. Contact an administrator, or try again when the allowance renews.' },
  scrape_failed: { no: 'Kunne ikke hente innhold fra siden. Prøv igjen.', en: 'Could not fetch content from the page. Try again.' },
  scrape_artifact_not_found: { no: 'Bildet eller PDF-filen er ikke lenger tilgjengelig.', en: 'The image or PDF is no longer available.' },
  scrape_artifact_unsupported: { no: 'Denne filtypen kan ikke vises eller lastes ned her.', en: 'This file type cannot be shown or downloaded here.' },
  scrape_artifact_failed: { no: 'Kunne ikke hente bildet eller PDF-filen. Prøv igjen.', en: 'Could not fetch the image or PDF. Try again.' },
  scrape_artifact_too_large: { no: 'Filen er for stor til å vises.', en: 'The file is too large to show.' },
  invalid_artifact: { no: 'Fil-referansen er ugyldig.', en: 'The file reference is not valid.' },
  artifact_not_found: { no: 'Filen er ikke lenger tilgjengelig.', en: 'The file is no longer available.' },
  artifact_unsupported: { no: 'Denne filtypen kan ikke vises her. Bare tekst kan åpnes.', en: 'This file type cannot be shown here. Only text can be opened.' },
  not_controllable: {
    no: 'Denne kjøringen går til den er ferdig, og kan ikke settes på pause, gjenopptas eller avbrytes.',
    en: 'This run goes to completion and cannot be paused, resumed or cancelled.',
  },
  upstream_invalid: { no: 'Nettleserøkten mangler id. Start den på nytt.', en: 'The browser session has no id. Start it again.' },
  feature_disabled: { no: 'Denne funksjonen er ikke slått på for organisasjonen din.', en: 'This feature is not enabled for your organization.' },
  browser_capability_unavailable: { no: 'Denne nettleserfunksjonen er ikke tilgjengelig akkurat nå.', en: 'This browser capability is not available right now.' },
  heavy_browser_busy: { no: 'Nettleseren er opptatt med en tung oppgave. Prøv igjen om litt.', en: 'The browser is busy with a heavy task. Try again shortly.' },
  action_failed: { no: 'Handlingen kunne ikke utføres.', en: 'The action could not be completed.' },
  capture_exceeds_inline_cap: { no: 'Opptaket er for stort til å sendes direkte. Prøv en mindre del av siden.', en: 'The capture is too large to send inline. Try a smaller part of the page.' },
  unsupported_media: { no: 'Dette innholdet kan ikke vises som forhåndsvisning. Prøv en lenke til en nettside eller et dokument.', en: 'This content cannot be previewed. Try a link to a web page or a document.' },
  extraction_failed: { no: 'Kunne ikke hente ut innhold fra dette dokumentet.', en: 'Could not extract content from this document.' },
  translation_failed: { no: 'Kunne ikke oversette innholdet.', en: 'Could not translate the content.' },
  sharepoint_register_failed: { no: 'Kunne ikke koble til SharePoint. Prøv igjen.', en: 'Could not connect to SharePoint. Try again.' },
  invalid_target_language: { no: 'Ugyldig målspråk valgt.', en: 'Invalid target language selected.' },
  studio_project_not_found: { no: 'Fant ikke prosjektet.', en: 'Project not found.' },
  fingerprint_unavailable: { no: 'Kunne ikke identifisere kilden akkurat nå.', en: 'Could not identify the source right now.' },

  // Integrations / delegated auth
  // Personal-connection ownership (owner ruling 5, security-personal-
  // connections, 2026-09-28): a personal Microsoft/Google connection (mail,
  // OneDrive/Drive, calendar, contacts) is usable only by the user who
  // connected it. integration-corev2 returns these two distinct codes for
  // "wrong user" vs "no user named at all" — surfaced from the connector-
  // attach picker (DashboardComposer.tsx's ConnectorAttachPicker) and any
  // other caller that reads through a connection.
  personal_connection_forbidden: {
    no: 'Denne tilkoblingen tilhører en annen bruker og kan ikke brukes her.',
    en: 'This connection belongs to a different user and cannot be used here.',
  },
  personal_connection_no_delegated_user: {
    no: 'Denne personlige tilkoblingen krever at du er logget inn som eieren.',
    en: 'This personal connection requires you to be signed in as its owner.',
  },
  delegated_auth_unavailable: { no: 'Denne integrasjonen er ikke koblet til ennå.', en: 'This integration is not connected yet.' },
  integration_auth_unavailable: { no: 'Denne integrasjonen er ikke koblet til ennå.', en: 'This integration is not connected yet.' },
  imports_auth_unavailable: { no: 'Import-tilkoblingen er ikke satt opp ennå.', en: 'The import connection is not set up yet.' },
  cost_auth_unavailable: { no: 'Kunne ikke hente kostnadsdata akkurat nå.', en: 'Could not fetch cost data right now.' },
  // Model Plane's pre-flight budget guard (model-gateway `budget.rs`): the one
  // hard stop on spend that exists. It refuses BEFORE the run starts, so the
  // copy must not imply a partial answer was cut off.
  budget_exceeded: {
    no: 'Forbrukstaket er nådd, så agenten startet ikke. En administrator kan endre taket under Innstillinger › Forbrukstak.',
    en: 'The spend ceiling has been reached, so the agent did not start. An administrator can change the ceiling under Settings › Spend ceiling.',
  },
  budget_unavailable: {
    no: 'Forbruket kunne ikke kontrolleres, så agenten startet ikke. Prøv igjen om litt.',
    en: 'Spend could not be verified, so the agent did not start. Try again shortly.',
  },
  shipping_auth_unavailable: { no: 'Fraktintegrasjonen er ikke koblet til ennå.', en: 'The shipping integration is not connected yet.' },
  social_core_unavailable: { no: 'Sosiale medier-tjenesten er utilgjengelig akkurat nå.', en: 'The social service is unavailable right now.' },
  search_provider_unconfigured: { no: 'Søketjenesten er ikke satt opp ennå.', en: 'The search provider is not configured yet.' },
  video_search_unavailable: { no: 'Videosøk er utilgjengelig akkurat nå.', en: 'Video search is unavailable right now.' },
  api_key_create_failed: { no: 'Kunne ikke opprette API-nøkkel. Prøv igjen.', en: 'Could not create API key. Try again.' },
  invalid_brand_theme: { no: 'Ugyldig temavalg.', en: 'Invalid theme selection.' },
  invalid_profile_scope: { no: 'Ugyldig tilgangsomfang for denne profilen.', en: 'Invalid scope for this profile.' },
  invalid_profile_update: { no: 'Kunne ikke oppdatere profilen med disse verdiene.', en: 'Could not update the profile with these values.' },

  // Browser control (advanced/internal surfaces — low pilot exposure, still safe to map)
  browser_human_takeover_active: { no: 'En person har overtatt styringen av denne nettleserøkten.', en: 'Human takeover is active for this browser session.' },
  browser_control_epoch_stale: { no: 'Styringen av nettleseren ble endret et annet sted. Oppdatert; prøv igjen.', en: 'Browser control changed elsewhere. It has been refreshed; try again.' },
  browser_capacity_reached: { no: 'Det er for mange nettleserøkter i gang akkurat nå.', en: 'There are too many browser sessions running right now.', retryable: true },
  browser_session_not_found: { no: 'Fant ikke nettleserøkten.', en: 'Browser session not found.' },
  browser_procedure_not_compilable: { no: 'Prosedyren kan ikke kompileres ennå. Fullfør flere handlinger og prøv igjen.', en: 'The procedure cannot be compiled yet. Complete more actions and try again.' },
  browser_receipts_unavailable_zdr: { no: 'Handlingsloggen er ikke tilgjengelig for økter uten lagring.', en: 'The action log is not available for sessions without retention.' },
  invalid_browser_evidence_query: { no: 'Filteret for nettleserloggen er ugyldig.', en: 'The browser log filter is not valid.' },
  invalid_devtools_query: { no: 'Filteret for konsoll og nettverk er ugyldig.', en: 'The console and network filter is not valid.' },
  browser_session_failed: { no: 'Nettleserøkten feilet. Prøv igjen.', en: 'The browser session failed. Try again.' },
  browser_session_invalid: { no: 'Denne nettleserøkten er ugyldig.', en: 'This browser session is invalid.' },
  browser_run_conflict: { no: 'En annen kjøring pågår allerede for denne økten.', en: 'Another run is already active for this session.' },
  browser_run_invalid: { no: 'Ugyldig nettleserkjøring.', en: 'Invalid browser run.' },
  browser_owner_unavailable: { no: 'Fant ikke eieren av denne økten.', en: 'Could not find the owner of this session.' },
  browser_store_unavailable: { no: 'Nettleserlagring er utilgjengelig akkurat nå.', en: 'Browser storage is unavailable right now.' },
  browser_artifact_failed: { no: 'Kunne ikke hente nettleserdata. Prøv igjen.', en: 'Could not fetch browser artifact. Try again.' },
  browser_artifact_too_large: { no: 'Nettleserdataene var for store.', en: 'The browser artifact was too large.' },
  browser_frame_failed: { no: 'Kunne ikke hente skjermbilde. Prøv igjen.', en: 'Could not fetch the frame. Try again.' },
  browser_frame_invalid: { no: 'Ugyldig skjermbilde.', en: 'Invalid frame.' },
  browser_observation_missing: { no: 'Ingen observasjon tilgjengelig for denne økten.', en: 'No observation available for this session.' },
  browser_visual_observation_invalid: { no: 'Ugyldig visuell observasjon.', en: 'Invalid visual observation.' },
  browser_ws_invalid_upstream: { no: 'Mistet forbindelsen til nettleserøkten. Prøv igjen.', en: 'Lost connection to the browser session. Try again.' },
  invalid_browser_artifact: { no: 'Ugyldige nettleserdata.', en: 'Invalid browser artifact.' },
  invalid_browser_frame: { no: 'Ugyldig skjermbilde.', en: 'Invalid frame.' },
  invalid_browser_goal: { no: 'Ugyldig mål for nettleserøkten.', en: 'Invalid goal for the browser session.' },
  invalid_browser_profile: { no: 'Ugyldig nettleserprofil.', en: 'Invalid browser profile.' },
  invalid_browser_run: { no: 'Ugyldig nettleserkjøring.', en: 'Invalid browser run.' },
  invalid_browser_session: { no: 'Ugyldig nettleserøkt.', en: 'Invalid browser session.' },
  invalid_browser_tab: { no: 'Ugyldig fane.', en: 'Invalid tab.' },
  invalid_browser_control_mode: { no: 'Ugyldig styringsmodus for nettleseren.', en: 'Browser control mode must be agent_control or human_takeover.' },

  // Browser sessions and agent runs (Quarry caps, grants, lost sessions, bot walls).
  // Written for the person at the screen: what happened and what they can do. A
  // `retryable` entry gets a "try again in N seconds" sentence appended from the
  // error's retry hint (or "try again shortly" without one), so its text does not
  // carry one itself. Raw Quarry codes (`browser_session_cap`, `session_reaped`,
  // `USER_TOOK_OVER`, `CHALLENGE_DETECTED`) are mapped as well as the gateway's own,
  // so a passthrough body never falls through to the generic message.
  browser_session_cap: { no: 'Alle nettleserplassene er i bruk akkurat nå.', en: 'All browser slots are in use right now.', retryable: true },
  browser_tab_limit_reached: { no: 'Nettleseren har for mange faner åpne. Lukk en fane du ikke trenger.', en: 'The browser has too many tabs open. Close a tab you do not need.', retryable: true },
  browser_page_cap: { no: 'Nettleseren har for mange faner åpne. Lukk en fane du ikke trenger.', en: 'The browser has too many tabs open. Close a tab you do not need.', retryable: true },
  browser_session_grant_inactive: {
    no: 'Tilgangen til denne nettleserøkten har utløpt eller er trukket tilbake, så AI-kjøringen kunne ikke starte. Åpne økten på nytt og prøv igjen.',
    en: 'Access for this browser session has expired or been withdrawn, so the AI run could not start. Reopen the session and try again.',
  },
  browser_session_grant_expired: {
    no: 'Tilgangen til denne nettleserøkten har utløpt. Åpne økten på nytt og prøv igjen.',
    en: 'Access for this browser session has expired. Reopen the session and try again.',
  },
  browser_session_grant_missing: {
    no: 'Denne nettleserøkten har ikke tilgang til AI-kjøring. Åpne økten på nytt for å starte en.',
    en: 'This browser session has no access for an AI run. Reopen the session to start one.',
  },
  browser_grant_not_derivable: {
    no: 'Tilgangen til denne nettleserøkten har utløpt eller er trukket tilbake, så AI-kjøringen kunne ikke starte. Åpne økten på nytt og prøv igjen.',
    en: 'Access for this browser session has expired or been withdrawn, so the AI run could not start. Reopen the session and try again.',
  },
  browser_host_not_grantable: {
    no: 'Denne adressen kan ikke åpnes i nettleseren i appen. Sjekk lenken, eller prøv en annen kilde.',
    en: 'This address cannot be opened in the in-app browser. Check the link, or try another source.',
  },
  browser_grant_denied: {
    no: 'Du har ikke tilgang til å åpne nettleseren for denne økten. Be en administrator om tilgang.',
    en: 'You do not have access to open the browser for this session. Ask an administrator for access.',
  },
  browser_grant_limit_reached: {
    no: 'For mange nettleserøkter er åpne. Lukk en du ikke bruker, og prøv igjen.',
    en: 'Too many browser sessions are open. Close one you are not using and try again.',
  },
  browser_grant_unavailable: { no: 'Nettlesertilgang er midlertidig utilgjengelig.', en: 'Browser access is temporarily unavailable.', retryable: true },
  browser_session_expired: { no: 'Denne nettleserøkten er avsluttet. Åpne den på nytt for å fortsette.', en: 'This browser session has ended. Reopen it to keep browsing.', retryable: false },
  browser_process_lost: {
    no: 'Nettleseren bak denne økten stoppet uventet. Start en ny økt for å fortsette.',
    en: 'The browser behind this session stopped unexpectedly. Start a new session to continue.',
  },
  browser_session_reaped: {
    no: 'Denne nettleserøkten ble lukket fordi den var inaktiv. Start en ny økt for å fortsette.',
    en: 'This browser session was closed because it was idle. Start a new session to continue.',
  },
  session_reaped: {
    no: 'Denne nettleserøkten ble lukket fordi den var inaktiv. Start en ny økt for å fortsette.',
    en: 'This browser session was closed because it was idle. Start a new session to continue.',
  },
  browser_action_denied: { no: 'Denne handlingen er ikke tillatt i nettleseren i appen.', en: 'This action is not allowed in the in-app browser.' },
  browser_artifact_withheld: {
    no: 'Zero Data Retention er på for denne kjøringen, så det ble ikke tatt noe skjermbilde.',
    en: 'Zero Data Retention is on for this run, so no screenshot was captured.',
  },
  browser_artifact_missing: { no: 'Skjermbildet er ikke lenger tilgjengelig.', en: 'The screenshot is no longer available.' },
  browser_unhealthy: {
    no: 'Nettleseren er midlertidig utilgjengelig fordi den ikke svarer som den skal. Prøv igjen om litt, og kontakt en administrator hvis det vedvarer.',
    en: 'The browser is temporarily unavailable because it is not responding properly. Try again shortly, and contact an administrator if it persists.',
  },
  USER_TOOK_OVER: {
    no: 'En person har overtatt nettleseren, så agenten kan ikke handle akkurat nå. Vent til styringen er gitt tilbake.',
    en: 'A person has taken over the browser, so the agent cannot act right now. Wait until control is handed back.',
  },
  browser_navigation_outside_scope: {
    no: 'Denne økten er begrenset til domenene i kjøringen. Åpne siden i en ny økt, eller ta over nettleseren selv.',
    en: 'This session is limited to the domains of its run. Open the page in a new session, or take over the browser yourself.',
  },
  browser_site_blocked: {
    no: 'Nettstedet stopper automatisk nettlesing. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site blocks automated browsing. Take over the browser yourself, or try another source.',
  },
  challenge_detected: {
    no: 'Nettstedet stopper automatisk nettlesing. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site blocks automated browsing. Take over the browser yourself, or try another source.',
  },
  CHALLENGE_DETECTED: {
    no: 'Nettstedet stopper automatisk nettlesing. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site blocks automated browsing. Take over the browser yourself, or try another source.',
  },

  // A keyed AI-run start (`Idempotency-Key`): the gateway's refusals of the key itself.
  invalid_idempotency_key: {
    no: 'Forespørselen om å starte kjøringen kunne ikke gjentas trygt. Prøv å starte den på nytt.',
    en: 'The request to start the run could not be repeated safely. Try starting it again.',
  },
  browser_idempotency_key_reused: {
    no: 'Denne forespørselen ble allerede brukt til en annen oppgave. Start kjøringen på nytt med den nye oppgaven.',
    en: 'This request was already used for a different task. Start the run again with the new task.',
  },
  browser_idempotency_key_run_ended: {
    no: 'Kjøringen fra denne forespørselen er allerede avsluttet. Start en ny kjøring hvis du vil gjøre det om igjen.',
    en: 'The run from this request has already ended. Start a new run if you want to do it again.',
  },
  browser_idempotency_unverified: {
    no: 'Vi fikk ikke bekreftet om kjøringen allerede er startet.',
    en: 'We could not confirm whether the run has already started.',
    retryable: true,
  },

  // Scrape preview refusals (gateway `knowledge/quarry.rs`, `classify_scrape_failure`).
  // `site_blocked` is only a Quarry-confirmed bot wall; a plain refusal is
  // `scrape_refused`. None of them offers a proxy.
  site_blocked: {
    no: 'Dette nettstedet stopper automatisert henting. Prøv en annen kilde, eller åpne siden selv i nettleseren.',
    en: 'This site blocks automated fetching. Try another source, or open the page yourself in the browser.',
  },
  scrape_refused: {
    no: 'Nettstedet avviste forespørselen om å hente siden. Siden kan kreve innlogging eller være stengt for automatisk henting. Prøv en annen kilde.',
    en: 'The site refused the request to fetch the page. It may require a sign-in or be closed to automatic fetching. Try another source.',
  },
  scrape_rate_limited: { no: 'Nettstedet mottar for mange forespørsler akkurat nå.', en: 'The site is receiving too many requests right now.', retryable: true },
  scrape_timeout: {
    no: 'Siden brukte for lang tid på å svare. Prøv en mer spesifikk underside, eller en annen kilde.',
    en: 'The page took too long to respond. Try a more specific sub-page, or another source.',
  },

  // Refusals the gateway answers before any upstream call: the address or the
  // request itself is wrong, so the person fixes it rather than retries.
  invalid_url: {
    no: 'Adressen ser ikke ut som en offentlig nettside. Sjekk lenken og prøv igjen.',
    en: 'The address does not look like a public web page. Check the link and try again.',
  },
  invalid_crawl_request: {
    no: 'Gjennomgangen kunne ikke startes med disse innstillingene. Sjekk adressen og innstillingene, og prøv igjen.',
    en: 'The crawl could not be started with these settings. Check the address and the settings, and try again.',
  },
  invalid_browser_action: {
    no: 'Nettleseren kunne ikke utføre denne handlingen. Sjekk adressen eller handlingen, og prøv igjen.',
    en: 'The browser could not carry out this action. Check the address or the action, and try again.',
  },
  // A saved scrape preview link that is not valid, and a scheduled run kind or action that this
  // installation has not turned on.
  invalid_scrape_artifact: {
    no: 'Lenken til den lagrede forhåndsvisningen er ikke gyldig. Hent siden på nytt.',
    en: 'The link to the saved preview is not valid. Fetch the page again.',
  },
  unsupported_schedule_kind: {
    no: 'Denne typen planlagt kjøring er ikke slått på her ennå.',
    en: 'This kind of scheduled run is not turned on here yet.',
  },
  schedule_action_unavailable: {
    no: 'Denne handlingen for planlagte kjøringer er ikke slått på her ennå.',
    en: 'This action for scheduled runs is not turned on here yet.',
  },
  zdr_persistent_profile_forbidden: {
    no: 'Denne organisasjonen lagrer ikke data, så nettleseren kan ikke bruke en varig profil. Bruk en midlertidig profil.',
    en: 'This organization does not retain data, so the browser cannot use a persistent profile. Use a temporary profile.',
  },
  model_gateway_unreachable: {
    no: 'Agenten er midlertidig utilgjengelig. Prøv igjen om litt, og kontakt en administrator hvis det vedvarer.',
    en: 'The agent is temporarily unavailable. Try again shortly, and contact an administrator if it persists.',
  },

  // Status-class messages for a code with no entry of its own.
  rate_limited: { no: 'Det kommer for mange forespørsler akkurat nå.', en: 'There are too many requests right now.', retryable: true },
  unauthorized: { no: 'Du må logge inn på nytt.', en: 'You need to sign in again.' },

  // Better Auth passthrough (proxied verbatim by the gateway; UPPER_SNAKE_CASE per Better Auth's own convention)
  EMAIL_NOT_VERIFIED: { no: 'Bekreft e-postadressen din for å fortsette.', en: 'Verify your email address to continue.' },
  INVALID_EMAIL_OR_PASSWORD: { no: 'Feil e-post eller passord.', en: 'Invalid email or password.' },
  INVALID_OTP: { no: 'Feil engangskode. Prøv igjen.', en: 'Invalid one-time code. Try again.' },
  INVITATION_NOT_FOUND: { no: 'Fant ikke invitasjonen. Be om en ny.', en: 'Invitation not found. Ask for a new one.' },
  plan_upgrade_required: { no: 'Oppgrader planen din for å fortsette.', en: 'Upgrade your plan to continue.' },
  entitlement_required: { no: 'Denne funksjonen er ikke inkludert i planen din.', en: 'This feature is not included in your plan.' },

  // The org Zero Data Retention switch (gateway `orgs/settings.rs`), refused
  // when no AI provider has a recorded zero-retention attestation, or when that
  // cannot be checked. The same two codes come back from the settings page and
  // from organization creation (the onboarding step has the same switch), so the
  // copy names the way forward for both: carry on without it and switch it on in
  // the settings once a provider is attested.
  zdr_no_attested_provider: {
    no: 'Zero Data Retention kan ikke slås på ennå: ingen AI-leverandør har en registrert nullbevaringsgodkjenning. Fortsett uten, og slå det på i innstillingene når en leverandør er godkjent.',
    en: 'Zero Data Retention cannot be turned on yet: no AI provider has a recorded zero-retention attestation. Carry on without it, and turn it on in the settings once a provider is attested.',
  },
  zdr_gate_unavailable: {
    no: 'Kunne ikke kontrollere hvilke leverandører som har registrert ZDR-godkjenning, så Zero Data Retention kan ikke slås på akkurat nå. Prøv igjen om litt, eller fortsett uten og slå det på senere.',
    en: 'We could not check which providers have a recorded ZDR attestation, so Zero Data Retention cannot be turned on right now. Try again shortly, or carry on without it and turn it on later.',
  },

  // Rooms (Spaces), agents, schedules and instructions (gateway `spaces.rs`, `agent_actions.rs`,

  // `orgs/instructions.rs`, `persona.rs`). Written for the person at the screen: what happened and

  // what they can do. A "nothing was altered" refusal says so, because the gateway guarantees it.
  space_not_found: {
    no: 'Fant ikke dette rommet, eller du har ikke tilgang til det. Gå tilbake til oversikten og velg et annet rom.',
    en: 'This space could not be found, or you do not have access to it. Go back to the overview and pick another space.',
  },
  space_read_not_authorized: {
    no: 'Du har ikke tilgang til å lese samtalen i dette rommet. Be en eier eller leder av rommet om tilgang.',
    en: 'You do not have access to read the conversation in this space. Ask an owner or manager of the space for access.',
  },
  space_lifecycle_unavailable: {
    no: 'Rommets status kunne ikke hentes akkurat nå. Prøv igjen om litt.',
    en: 'The status of this space could not be fetched right now. Try again shortly.',
  },
  space_deletion_unavailable: {
    no: 'Slettingen av rommet kunne ikke kontrolleres akkurat nå. Prøv igjen om litt.',
    en: 'The deletion of this space could not be checked right now. Try again shortly.',
  },
  space_deletion_not_found: {
    no: 'Fant ingen slettingsforespørsel for dette rommet. Den kan allerede være fullført eller avbrutt.',
    en: 'No deletion request was found for this space. It may already be finished or cancelled.',
  },
  space_provisioning_unavailable: {
    no: 'Rommet kunne ikke opprettes akkurat nå, og ingenting ble satt opp. Prøv igjen om litt.',
    en: 'The space could not be created right now, and nothing was set up. Try again shortly.',
  },
  space_required: {
    no: 'Du trenger et aktivt personlig rom for å opprette en tidsplan.',
    en: 'You need an active personal space to create a schedule.',
  },
  space_role_cannot_create_agent: {
    no: 'Bare eiere og ledere av rommet kan legge til en agent her.',
    en: 'Only owners and managers of the space can add an agent here.',
  },
  space_role_cannot_edit_instructions: {
    no: 'Bare redaktører, ledere og eiere av rommet kan endre instruksjonene.',
    en: 'Only editors, managers and owners of the space can change its instructions.',
  },
  room_name_required: {
    no: 'Et rom trenger et navn. Skriv inn et navn og prøv igjen.',
    en: 'A room needs a name. Enter a name and try again.',
  },
  room_membership_unavailable: {
    no: 'Medlemmene i rommet kunne ikke endres akkurat nå, og ingenting ble endret. Prøv igjen om litt.',
    en: 'The members of the room could not be changed right now, and nothing was altered. Try again shortly.',
  },
  room_membership_unconfirmed: {
    no: 'Endringen ble lagret, men er ikke bekreftet for rommets medlemmer ennå. Det kan ta et øyeblikk før listen er oppdatert.',
    en: 'The change was saved, but the room members have not confirmed it yet. It may take a moment before the list is up to date.',
  },
  member_required: {
    no: 'Velg et medlem først.',
    en: 'Choose a member first.',
  },
  member_not_found: {
    no: 'Fant ikke dette medlemmet i organisasjonen.',
    en: 'This member was not found in the organization.',
  },
  thread_presentation_owner_only: {
    no: 'Bare den som startet innlegget kan endre tittelen eller feste det.',
    en: 'Only the person who started the post can retitle or pin it.',
  },
  thread_presentation_unavailable: {
    no: 'Innlegget kunne ikke endres, og ingenting ble endret. Prøv igjen.',
    en: 'The post could not be changed, and nothing was altered. Try again.',
  },
  invalid_presentation: {
    no: 'Tittelen må være mellom 1 og 200 tegn. Sjekk den og prøv igjen.',
    en: 'The title must be between 1 and 200 characters. Check it and try again.',
  },
  presence_unavailable: {
    no: 'Vi kunne ikke se hvem som er i rommet akkurat nå. Resten fungerer som vanlig.',
    en: 'We could not see who is in the room right now. Everything else works as usual.',
  },
  read_marker_unavailable: {
    no: 'Vi kunne ikke lagre hvor langt du har lest. Resten fungerer som vanlig.',
    en: 'We could not save how far you have read. Everything else works as usual.',
  },
  scoped_append_content_required: {
    no: 'Meldingen kan ikke være tom. Skriv noe og prøv igjen.',
    en: 'The message cannot be empty. Write something and try again.',
  },
  active_membership_required: {
    no: 'Du må være aktivt medlem av organisasjonen for å gjøre dette.',
    en: 'You need to be an active member of the organization to do this.',
  },
  owner_grant_role_required: {
    no: 'Bare en eier eller administrator i organisasjonen kan styre hva agenter har lov til å gjøre.',
    en: 'Only an owner or administrator of the organization can manage what agents are allowed to do.',
  },
  invalid_owner_grant_decision: {
    no: 'Tilgangskontrollen ga et uventet svar, så ingenting ble endret. Prøv igjen om litt.',
    en: 'The access check gave an unexpected answer, so nothing was changed. Try again shortly.',
  },
  invalid_space_decision: {
    no: 'Tilgangskontrollen ga et uventet svar, så rommet ble ikke åpnet. Prøv igjen om litt.',
    en: 'The access check gave an unexpected answer, so the space was not opened. Try again shortly.',
  },
  owner_action_contract_invalid: {
    no: 'Samtaletjenesten ga et uventet svar, så handlingen ble ikke utført. Prøv igjen om litt.',
    en: 'The conversation service gave an unexpected answer, so the action was not carried out. Try again shortly.',
  },
  ticket_operation_receipt_invalid: {
    no: 'Vi fikk ikke bekreftet at endringen på saken ble lagret. Last inn siden på nytt for å se hva som skjedde.',
    en: 'We could not confirm that the change to the ticket was saved. Reload the page to see what happened.',
  },
  invalid_model_gateway_response: {
    no: 'Fikk et uventet svar fra AI-tjenesten. Prøv igjen.',
    en: 'Got an unexpected answer from the AI service. Try again.',
  },
  agent_binding_state_unavailable: {
    no: 'Agentens status i dette rommet kunne ikke endres, og ingenting ble endret. Prøv igjen.',
    en: 'The status of the agent in this room could not be changed, and nothing was altered. Try again.',
  },
  agent_binding_unavailable: {
    no: 'Agenten kunne ikke knyttes til rommet, og ingenting ble satt opp. Prøv igjen om litt.',
    en: 'The agent could not be added to the room, and nothing was set up. Try again shortly.',
  },
  agent_creation_unavailable: {
    no: 'Agenten kunne ikke opprettes, og ingenting ble satt opp. Prøv igjen om litt.',
    en: 'The agent could not be created, and nothing was set up. Try again shortly.',
  },
  agent_invocation_blocked: {
    no: 'Agenten er satt på pause eller fjernet i dette rommet, så den kan ikke brukes nå.',
    en: 'The agent is paused or removed in this room, so it cannot be used now.',
  },
  agent_membership_unconfirmed: {
    no: 'Agenten ble lagt til, men rommet har ikke bekreftet den som medlem ennå. Den står som ventende i agentfanen.',
    en: 'The agent was added, but the room has not confirmed it as a member yet. It stays pending in the agents tab.',
  },
  agent_mention_not_enabled: {
    no: 'Denne agenten kan ikke tilkalles med @ i dette rommet.',
    en: 'This agent cannot be called with @ in this room.',
  },
  agent_mention_requires_space: {
    no: 'En agent kan bare tilkalles med @ inne i et rom.',
    en: 'An agent can only be called with @ inside a room.',
  },
  mentioned_agent_not_bound: {
    no: 'Denne agenten er ikke lagt til i dette rommet.',
    en: 'This agent has not been added to this room.',
  },
  agent_registry_unavailable: {
    no: 'Organisasjonens agenter kunne ikke hentes akkurat nå. Prøv igjen om litt.',
    en: 'The organization agents could not be fetched right now. Try again shortly.',
  },
  agent_revocation_unconfirmed: {
    no: 'Agenten ble fjernet, men rommets medlemsliste er ikke oppdatert ennå. Den kan vises en stund til.',
    en: 'The agent was removed, but the member list of the room is not updated yet. It may still show for a while.',
  },
  installable_agents_unavailable: {
    no: 'Listen over agenter du kan legge til kunne ikke hentes akkurat nå. Prøv igjen om litt.',
    en: 'The list of agents you can add could not be fetched right now. Try again shortly.',
  },
  invalid_agent_instructions: {
    no: 'Instruksjonene til agenten må være under 4000 tegn.',
    en: 'The agent instructions must be under 4000 characters.',
  },
  invalid_agent_name: {
    no: 'Navnet på agenten må være mellom 2 og 60 tegn.',
    en: 'The agent name must be between 2 and 60 characters.',
  },
  instructions_too_long: {
    no: 'Instruksjonene er for lange. De kan ha opptil 4000 tegn.',
    en: 'The instructions are too long. They can have up to 4000 characters.',
  },
  instructions_unavailable: {
    no: 'Instruksjonene kunne ikke lagres akkurat nå. Prøv igjen om litt.',
    en: 'The instructions could not be saved right now. Try again shortly.',
  },
  persona_instructions_too_long: {
    no: 'Instruksjonene er for lange. De kan ha opptil 1500 tegn.',
    en: 'The instructions are too long. They can have up to 1500 characters.',
  },
  invalid_cron_request: {
    no: 'Tidsplanen kunne ikke opprettes med disse innstillingene. Sjekk oppgaven og tidspunktet, og prøv igjen.',
    en: 'The schedule could not be created with these settings. Check the task and the time, and try again.',
  },
  invalid_task_template: {
    no: 'Oppgaven i tidsplanen kunne ikke brukes slik den er. Sjekk oppgaven, og prøv igjen.',
    en: 'The task in the schedule could not be used as it is. Check the task, and try again.',
  },
  task_template_required: {
    no: 'En tidsplan trenger en oppgave. Beskriv oppgaven og prøv igjen.',
    en: 'A schedule needs a task. Describe the task and try again.',
  },

  // Inbox, tickets and the organization's support and data-retention rules (gateway `inbox.rs`,

  // `tickets.rs`, `chat/history.rs`). The ZDR refusals say what is not kept, not that it failed.
  active_organization_required: {
    no: 'Velg en organisasjon først.',
    en: 'Select an organization first.',
  },
  ai_review_mode_required: {
    no: 'Lagrede AI-forslag krever at organisasjonen bruker gjennomgang av AI i support. En administrator kan slå det på i innstillingene.',
    en: 'Saved AI proposals need the organization to use AI review in support. An administrator can turn it on in the settings.',
  },
  zdr_ai_proposal_forbidden: {
    no: 'AI-forslag lagres ikke, fordi organisasjonen har Zero Data Retention slått på.',
    en: 'AI proposals are not kept, because the organization has Zero Data Retention turned on.',
  },
  zdr_draft_persistence_forbidden: {
    no: 'Utkast lagres ikke, fordi organisasjonen har Zero Data Retention slått på.',
    en: 'Drafts are not kept, because the organization has Zero Data Retention turned on.',
  },
  zdr_recurrence_forbidden: {
    no: 'Lignende saker vises ikke, fordi organisasjonen har Zero Data Retention slått på.',
    en: 'Similar tickets are not shown, because the organization has Zero Data Retention turned on.',
  },
  support_ai_policy_unavailable: {
    no: 'AI-gjennomgang er utilgjengelig til organisasjonens regler kan kontrolleres. Prøv igjen om litt.',
    en: 'AI review is unavailable until the organization rules can be checked. Try again shortly.',
  },
  retention_posture_unavailable: {
    no: 'Utkast kan ikke lagres før organisasjonens regler for datalagring kan kontrolleres. Prøv igjen om litt.',
    en: 'Drafts cannot be saved until the organization data-retention rules can be checked. Try again shortly.',
  },
  support_recurrence_permission_required: {
    no: 'Rollen din har ikke tilgang til å se lignende saker.',
    en: 'Your role does not have access to see similar tickets.',
  },
  support_recurrence_policy_unavailable: {
    no: 'Lignende saker er utilgjengelige til organisasjonens regler og tilganger kan kontrolleres. Prøv igjen om litt.',
    en: 'Similar tickets are unavailable until the organization rules and permissions can be checked. Try again shortly.',
  },
  thread_not_found: {
    no: 'Fant ikke samtalen. Den kan være slettet.',
    en: 'The conversation was not found. It may have been deleted.',
  },

  // Knowledge, SharePoint and search (gateway `knowledge/*.rs`, `search.rs`, `insights.rs`).
  sharepoint_sites_unavailable: {
    no: 'Vi kunne ikke hente SharePoint-nettstedene. Koble til Microsoft 365 først, og prøv igjen.',
    en: 'We could not fetch the SharePoint sites. Connect Microsoft 365 first, and try again.',
  },
  sharepoint_drives_unavailable: {
    no: 'Vi kunne ikke hente dokumentbibliotekene for dette nettstedet. Prøv igjen om litt.',
    en: 'We could not fetch the document libraries for this site. Try again shortly.',
  },
  sharepoint_folders_unavailable: {
    no: 'Vi kunne ikke hente mappene i dette dokumentbiblioteket. Prøv igjen om litt.',
    en: 'We could not fetch the folders in this document library. Try again shortly.',
  },
  site_id_required: {
    no: 'Velg et SharePoint-nettsted først.',
    en: 'Choose a SharePoint site first.',
  },
  drive_id_required: {
    no: 'Velg et dokumentbibliotek først.',
    en: 'Choose a document library first.',
  },
  invalid_content: {
    no: 'Innholdet kan ikke være tomt. Skriv eller lim inn noe og prøv igjen.',
    en: 'The content cannot be empty. Write or paste something and try again.',
  },
  invalid_body: {
    no: 'Dokumentet trenger både en tittel og et innhold. Fyll ut begge og prøv igjen.',
    en: 'The document needs both a title and content. Fill in both and try again.',
  },
  no_products: {
    no: 'Velg minst ett produkt som skal oppsummeres.',
    en: 'Choose at least one product to summarize.',
  },
  summary_failed: {
    no: 'Vi kunne ikke lage AI-sammendraget. Prøv igjen.',
    en: 'We could not create the AI summary. Try again.',
  },
  invalid_query: {
    no: 'Skriv inn et søk og prøv igjen.',
    en: 'Enter a search and try again.',
  },
  web_search_unavailable: {
    no: 'Nettsøket kunne ikke fullføres akkurat nå. Prøv igjen om litt.',
    en: 'The web search could not be completed right now. Try again shortly.',
  },
  web_search_timeout: {
    no: 'Nettsøket brukte for lang tid. Prøv igjen, eller gjør søket mer spesifikt.',
    en: 'The web search took too long. Try again, or make the search more specific.',
  },
  video_search_timeout: {
    no: 'Videosøket brukte for lang tid. Prøv igjen, eller gjør søket mer spesifikt.',
    en: 'The video search took too long. Try again, or make the search more specific.',
  },
  insights_upstream_unavailable: {
    no: 'Innsikt er midlertidig utilgjengelig. Prøv igjen om litt.',
    en: 'Insights is temporarily unavailable. Try again shortly.',
  },
  // A screenshot or a file of a browser run, relayed as bytes (`upstream.rs`, `proxy_artifact_bytes`).
  artifact_unavailable: {
    no: 'Vi kunne ikke hente vedlegget akkurat nå. Prøv igjen om litt.',
    en: 'We could not fetch the attachment right now. Try again shortly.',
  },
  artifact_too_large: {
    no: 'Vedlegget er for stort til å vises.',
    en: 'The attachment is too large to display.',
  },

  // Onboarding and the website preview. The three crawl codes arrive as `warning` events on the

  // preview stream, not as an error response; the onboarding page translates them by code.
  connection_not_found: {
    no: 'Vi fant ingen tilkobling for denne tjenesten ennå. Koble den til først, og prøv igjen.',
    en: 'We found no connection for this service yet. Connect it first, and try again.',
  },
  invalid_name: {
    no: 'Organisasjonen trenger et navn. Skriv inn et navn og prøv igjen.',
    en: 'The organization needs a name. Enter a name and try again.',
  },
  onboarding_completion_failed: {
    no: 'Oppsettet kunne ikke fullføres trygt. Fremgangen din er lagret, så du kan prøve igjen.',
    en: 'The setup could not be completed safely. Your progress is saved, so you can try again.',
  },
  recommendation_unavailable: {
    no: 'Planforslaget er midlertidig utilgjengelig. Velg en plan selv, eller prøv igjen senere.',
    en: 'The plan recommendation is temporarily unavailable. Choose a plan yourself, or try again later.',
  },
  translation_unavailable: {
    no: 'Oversettelsen er utilgjengelig akkurat nå. Prøv igjen om litt.',
    en: 'The translation is unavailable right now. Try again shortly.',
  },
  translation_invalid_response: {
    no: 'Oversettelsen ga et uventet svar. Prøv igjen om litt.',
    en: 'The translation gave an unexpected answer. Try again shortly.',
  },
  empty_crawl: {
    no: 'Vi fikk ikke hentet innhold fra nettsiden. Prøv på nytt, eller fortsett uten forhåndsvisning.',
    en: 'We could not get any content from the website. Try again, or continue without a preview.',
  },
  control_unreachable: {
    no: 'Vi kunne ikke følge med på hentingen av nettsiden. Prøv på nytt, eller fortsett uten forhåndsvisning.',
    en: 'We could not follow the fetching of the website. Try again, or continue without a preview.',
  },
  crawl_failed: {
    no: 'Hentingen av nettsiden mislyktes. Prøv på nytt, eller fortsett uten forhåndsvisning.',
    en: 'Fetching the website failed. Try again, or continue without a preview.',
  },
  invalid_checkout_request: {
    no: 'Betalingen kunne ikke startes med disse opplysningene. Sjekk dem og prøv igjen.',
    en: 'The payment could not be started with these details. Check them and try again.',
  },

  // Session, settings and integrations (gateway `middleware.rs`, `settings/*.rs`, `navbar.rs`, `upstream.rs`).
  session_verification_unavailable: {
    no: 'Innloggingen din kunne ikke bekreftes akkurat nå, men økten er ikke endret. Prøv igjen om litt.',
    en: 'Your sign-in could not be verified right now, but your session is unchanged. Try again shortly.',
  },
  api_key_error: {
    no: 'API-nøkkelen kunne ikke endres. Prøv igjen.',
    en: 'The API key could not be changed. Try again.',
  },
  invalid_api_key_name: {
    no: 'API-nøkkelen trenger et navn. Skriv inn et navn og prøv igjen.',
    en: 'The API key needs a name. Enter a name and try again.',
  },
  invalid_color_scheme: {
    no: 'Fargevalget er ikke gyldig. Velg et fargetema, eller skriv en gyldig fargekode.',
    en: 'The colour choice is not valid. Pick a colour theme, or enter a valid colour code.',
  },
  invalid_theme: {
    no: 'Temaet er ikke gyldig. Velg lyst, mørkt eller systemets tema.',
    en: 'The theme is not valid. Choose light, dark or the system theme.',
  },
  invalid_request: {
    no: 'Forespørselen mangler noe eller er ugyldig. Sjekk og prøv igjen.',
    en: 'The request is missing something or is not valid. Check it and try again.',
  },
  no_sources_registered: {
    no: 'Ingen kilder er valgt for denne tilkoblingen ennå. Velg et bibliotek først.',
    en: 'No sources are chosen for this connection yet. Choose a library first.',
  },
  PLAN_REQUIRED: {
    no: 'Denne funksjonen krever en høyere plan. Oppgrader planen din for å fortsette.',
    en: 'This feature needs a higher plan. Upgrade your plan to continue.',
  },
  model_error: {
    no: 'AI-tjenesten svarte med en feil. Prøv igjen.',
    en: 'The AI service returned an error. Try again.',
  },
  model_request_timeout: {
    no: 'AI-tjenesten brukte for lang tid på å svare. Prøv igjen.',
    en: 'The AI service took too long to answer. Try again.',
  },

  // Sign-in and invitations: auth-core's own codes, passed through verbatim (UPPER_SNAKE_CASE).
  INVITATION_AUTHORITY_UNAVAILABLE: {
    no: 'Invitasjonen kunne ikke kontrolleres akkurat nå. Prøv igjen om litt.',
    en: 'The invitation could not be checked right now. Try again shortly.',
  },
  TOO_MANY_REQUESTS: {
    no: 'Det kommer for mange forsøk akkurat nå.',
    en: 'There are too many attempts right now.',
    retryable: true,
  },
  USER_ALREADY_EXISTS: {
    no: 'Det finnes allerede en konto med denne e-postadressen. Logg inn i stedet.',
    en: 'An account with this email address already exists. Sign in instead.',
  },
  DISPOSABLE_EMAIL_BLOCKED: {
    no: 'Engangsadresser kan ikke brukes. Bruk din vanlige e-postadresse.',
    en: 'Disposable email addresses cannot be used. Use your regular email address.',
  },

  // What Quarry answers with on a browser call and the gateway passes through unchanged (its own

  // UPPER_SNAKE_CASE codes; `CHALLENGE_DETECTED` and `USER_TOOK_OVER` are above). The generic ones

  // (BAD_REQUEST, UNAUTHORIZED, FORBIDDEN, NOT_FOUND, CONFLICT, INTERNAL, ACTION_UNKNOWN) answer by

  // status class instead, so they are not listed.
  RATE_LIMITED: {
    no: 'Det kommer for mange forespørsler akkurat nå.',
    en: 'There are too many requests right now.',
    retryable: true,
  },
  TIMEOUT: {
    no: 'Nettleseren brukte for lang tid på å svare. Prøv igjen.',
    en: 'The browser took too long to respond. Try again.',
  },
  SECURITY_BLOCKED: {
    no: 'Nettleseren i appen stoppet denne handlingen av sikkerhetsgrunner. Prøv en annen adresse eller handling.',
    en: 'The in-app browser stopped this action for security reasons. Try another address or action.',
  },
  DRIVER_FAILED: {
    no: 'Nettleseren kunne ikke utføre handlingen. Prøv igjen, eller start en ny økt.',
    en: 'The browser could not carry out the action. Try again, or start a new session.',
  },
  RUNTIME_NOT_READY: {
    no: 'Nettleseren er ikke klar ennå.',
    en: 'The browser is not ready yet.',
    retryable: true,
  },
  CHECKPOINT_LOST: {
    no: 'Siden endret seg underveis, så agenten mistet tråden. Start kjøringen på nytt.',
    en: 'The page changed along the way, so the agent lost its place. Start the run again.',
  },
  TARGET_REPAIR_REQUIRED: {
    no: 'Siden endret seg, og agenten fant ikke igjen stedet den skulle trykke. Prøv handlingen på nytt.',
    en: 'The page changed, and the agent could not find the place to click again. Try the action again.',
  },
  UPSTREAM_BLOCKED: {
    no: 'Nettstedet svarte ikke eller stengte tilgangen. Prøv en annen kilde.',
    en: 'The site did not answer or closed access. Try another source.',
  },
  UNSUPPORTED: {
    no: 'Nettleseren i appen støtter ikke dette ennå.',
    en: 'The in-app browser does not support this yet.',
  },
  UNSUPPORTED_MEDIA: {
    no: 'Dette innholdet kan ikke åpnes i nettleseren i appen.',
    en: 'This content cannot be opened in the in-app browser.',
  },
  BUDGET_EXHAUSTED: {
    no: 'Grensen for hva nettleseren kan bruke i denne kjøringen er nådd. Start en ny kjøring.',
    en: 'The limit on what the browser may use in this run has been reached. Start a new run.',
  },
}

/**
 * Why a bot wall stands in front of a page: the closed `BlockReason` set Quarry
 * sends (`block_reason` on a scrape, `reason` on an agent run), keyed by the
 * wire value. Used instead of the generic site-blocked copy when a refusal
 * carries one; a value that is not here is ignored, never shown.
 */
export const BLOCK_REASON_COPY: Record<string, ErrorCopy> = {
  captcha_widget: {
    no: 'Siden krever at du bekrefter at du er et menneske, og agenten kan ikke gjøre det for deg. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The page asks you to prove you are a person, and the agent cannot do that for you. Take over the browser yourself, or try another source.',
  },
  waf_interstitial: {
    no: 'Nettstedet viser en sikkerhetskontroll i stedet for siden. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site shows a security check instead of the page. Take over the browser yourself, or try another source.',
  },
  unexpected_bot_error: {
    no: 'Nettstedet tok den automatiske hentingen for en robot og avviste den. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site took the automatic fetch for a robot and turned it away. Take over the browser yourself, or try another source.',
  },
  challenge_loop: {
    no: 'Nettstedet stoppet agenten igjen etter ett nytt forsøk, så den ga opp. Ta over nettleseren selv, eller prøv en annen kilde.',
    en: 'The site stopped the agent again after one more try, so it gave up. Take over the browser yourself, or try another source.',
  },
}

const BLOCKED_CODES = new Set(['site_blocked', 'browser_site_blocked', 'challenge_detected', 'CHALLENGE_DETECTED'])
const TAKEOVER_CODES = new Set(['USER_TOOK_OVER', 'browser_human_takeover_active'])
const SESSION_LOST_CODES = new Set(['browser_process_lost', 'browser_session_reaped', 'session_reaped'])

const NETWORK_COPY: ErrorCopy = {
  no: 'Fikk ikke kontakt med tjenesten. Sjekk nettverket ditt og prøv igjen.',
  en: 'Could not reach the service. Check your network and try again.',
}

/**
 * What a caller needs to react to an error, beyond its text:
 *  - `unauthenticated` (401): sign in again.
 *  - `forbidden` (403): not allowed; ask an administrator.
 *  - `takeover` (409 `USER_TOOK_OVER` / `browser_human_takeover_active`): a person
 *    holds the browser; wait, do not retry the agent step.
 *  - `blocked`: the site refuses automation; take over or use another source.
 *  - `rate_limited` (429): wait `retryAfterSeconds`, then retry.
 *  - `session_lost` (410 or a reaped/lost session): start a new session.
 *  - `server` (5xx): a backing service failed; retry later.
 *  - `network`: no response at all (fetch failed).
 *  - `client`: any other 4xx, or an error with no HTTP status.
 */
export type ApiErrorKind =
  | 'unauthenticated'
  | 'forbidden'
  | 'takeover'
  | 'blocked'
  | 'rate_limited'
  | 'session_lost'
  | 'server'
  | 'network'
  | 'client'

export type ApiErrorDescription = {
  code: string | null
  kind: ApiErrorKind
  message: string
  resumable: boolean
  retryAfterSeconds: number | null
}

function errorKind(err: ApiErrorLike, code: string | null): ApiErrorKind {
  if (code && TAKEOVER_CODES.has(code)) return 'takeover'
  if (code && BLOCKED_CODES.has(code)) return 'blocked'
  const status = typeof err?.status === 'number' ? err.status : null
  if ((code && SESSION_LOST_CODES.has(code)) || status === 410) return 'session_lost'
  if (status === 401) return 'unauthenticated'
  if (status === 403) return 'forbidden'
  if (status === 429) return 'rate_limited'
  if (status !== null && status >= 500) return 'server'
  if (status === null && err instanceof TypeError) return 'network'
  return 'client'
}

function retrySentence(seconds: number | null, tr: (noText: string, enText: string) => string): string {
  if (seconds === null) return tr('Prøv igjen om litt.', 'Try again shortly.')
  return seconds === 1
    ? tr('Prøv igjen om 1 sekund.', 'Try again in 1 second.')
    : tr(`Prøv igjen om ${seconds} sekunder.`, `Try again in ${seconds} seconds.`)
}

function describe(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback: ErrorCopy | undefined,
): ApiErrorDescription {
  const like = (err && typeof err === 'object' ? err : null) as ApiErrorLike
  const code = typeof like?.code === 'string' ? like.code : null
  const kind = errorKind(like, code)
  const retryAfterSeconds =
    typeof like?.retryAfterSeconds === 'number' && like.retryAfterSeconds >= 1 ? Math.ceil(like.retryAfterSeconds) : null

  let copy: ErrorCopy | undefined
  if (err instanceof LocalizedError) copy = err.copy
  // A refusal that names why the wall is up gets the sentence for that reason.
  if (kind === 'blocked' && typeof like?.blockReason === 'string') copy = BLOCK_REASON_COPY[like.blockReason]
  copy ??= code ? ERROR_COPY[code] : undefined
  if (!copy) {
    if (kind === 'unauthenticated') copy = ERROR_COPY.unauthenticated
    else if (kind === 'forbidden') copy = ERROR_COPY.forbidden
    else if (kind === 'rate_limited') copy = ERROR_COPY.rate_limited
    else if (kind === 'network') copy = NETWORK_COPY
  }
  copy ??= fallback ?? GENERIC_FALLBACK

  const base = tr(copy.no, copy.en)
  return {
    code,
    kind,
    message: copy.retryable === true ? `${base} ${retrySentence(retryAfterSeconds, tr)}` : base,
    resumable: like?.resumable === true,
    retryAfterSeconds,
  }
}

/**
 * Translate a caught error into safe, locale-aware copy. Looks the error's
 * `code` up in {@link ERROR_COPY}; unknown/missing codes use `fallback` if the
 * caller supplied one, otherwise a generic Norwegian/English message — never
 * the raw `message` from the gateway, which is always English ops text. A
 * retryable entry gets a "try again in N seconds" sentence from the error's own
 * retry hint. For an error whose code has no copy it answers by status class
 * (401, 403, 429, no network) before the caller's `fallback`: a session that
 * expired, a refusal, a rate limit or a lost connection is never reported as the
 * fallback's "could not do X", the same as {@link describeApiError}.
 */
export function translateApiError(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback?: ErrorCopy,
): string {
  return describe(err, tr, fallback).message
}

/**
 * {@link translateApiError} plus what the caller needs to react: the `kind`
 * (401, 403, takeover, bot wall, 429, lost session, 5xx, no network), the retry
 * hint and `resumable`. Answers by status class exactly as {@link translateApiError}
 * does. Still never the raw `message`.
 */
export function describeApiError(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback?: ErrorCopy,
): ApiErrorDescription {
  return describe(err, tr, fallback)
}
