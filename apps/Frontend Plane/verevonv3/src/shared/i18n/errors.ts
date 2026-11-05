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
// The zero-retention refusals of the Model Plane catalogue are the exception:
// their words live with the catalogue (shared/api/zdr-refusals.ts), so this seam
// renders them from there. The model-entitlement refusals do the same
// (shared/api/model-entitlement-refusals.ts).

import { isModelEntitlementCode, modelEntitlementPresentation } from '@/shared/api/model-entitlement-refusals'
import { isSpaceDenial, presentSpaceDenialText } from '@/shared/api/space-denials'
import { en as enStrings } from './en'
import { no as noStrings } from './no'

import { isZdrRefusalCode, zdrRefusalPresentation } from '@/shared/api/zdr-refusals'

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
      feature?: string | null
      details?: Record<string, unknown> | null
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
  fork_unavailable: { no: 'Kunne ikke lage en kopi av tråden akkurat nå.', en: 'Could not copy the thread right now.' },
  upstream_shape_error: { no: 'Fikk et uventet svar fra tjenesten. Prøv igjen.', en: 'Got an unexpected response from the service. Try again.' },
  internal_error: { no: 'Forespørselen kunne ikke fullføres. Prøv igjen, eller kontakt support hvis problemet vedvarer.', en: 'The request could not be completed. Try again, or contact support if the problem persists.' },
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
  thread_run_active: { no: 'Agenten jobber allerede i denne samtalen. Meldingen er lagt ut. Prøv igjen når den er ferdig.', en: 'An agent is already working in this conversation. Your message was posted. Try again when it finishes.' },
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
  approval_required: { no: 'Gjennomgangen er for stor til å starte uten godkjenning. Bekreft antall sider og start den på nytt.', en: 'The crawl is too large to start without approval. Confirm the number of pages and start it again.' },
  crawl_approval_required: { no: 'Agenten kan ikke starte en stor gjennomgang, eller en uten sidegrense, uten at du godkjenner den først.', en: 'The agent cannot start a large crawl, or one without a page limit, until you approve it.' },
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
  invalid_job_id: { no: 'Oppgaven er ikke gyldig. Last inn siden på nytt.', en: 'The task is not valid. Reload the page.' },
  upstream_invalid: { no: 'Nettleserøkten mangler id. Start den på nytt.', en: 'The browser session has no id. Start it again.' },
  extract_ai_formats_off: {
    no: 'Strukturert uttrekk er slått av på Quarry-edge (QUARRY_EDGE__AI_FORMATS).',
    en: 'Structured extraction is switched off in Quarry-edge (QUARRY_EDGE__AI_FORMATS).',
  },
  extract_model_plane_not_configured: {
    no: 'Modellplanen er ikke koblet til Quarry-edge (QUARRY_EDGE__MODEL_PLANE_URL mangler).',
    en: 'The model plane is not connected to Quarry-edge (QUARRY_EDGE__MODEL_PLANE_URL is not set).',
  },
  extract_model_lane_not_granted: {
    no: 'Denne installasjonen har ingen modellnøkkel for strukturert uttrekk.',
    en: 'This installation has no model credential for structured extraction.',
  },
  extract_model_plane_refused: {
    no: 'Modellplanen avviste Quarry-edge ved første modellkall. Det ble ikke belastet noe.',
    en: 'The model plane refused Quarry-edge on the first model call. Nothing was metered.',
  },
  feature_disabled: {
    no: 'Denne funksjonen er ikke slått på for deg ennå.',
    en: 'This feature is not turned on for you yet.',
  },
  browser_capability_unavailable: { no: 'Denne nettleserfunksjonen er ikke tilgjengelig akkurat nå.', en: 'This browser capability is not available right now.' },
  heavy_browser_busy: { no: 'Nettleseren er opptatt med en tung oppgave. Prøv igjen om litt.', en: 'The browser is busy with a heavy task. Try again shortly.' },
  action_failed: { no: 'Handlingen kunne ikke utføres.', en: 'The action could not be completed.' },
  too_large: { no: 'Dokumentet er større enn 25 MiB. Velg en mindre fil.', en: 'The document is larger than 25 MiB. Choose a smaller file.' },
  upstream_timeout: { no: 'Lesingen tok for lang tid. Prøv med et kortere dokument.', en: 'Reading took too long. Try a shorter document.' },
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
  // W3-E3.4-5: an input frame on the owner's view-only live socket (gateway browser.rs).
  browser_live_view_only: {
    no: 'Denne visningen er bare for å se på. Bruk den vanlige nettleservisningen for å styre siden.',
    en: 'This view is for watching only. Use the regular browser view to control the page.',
  },
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
  browser_attachment_refused_zdr: { no: 'Deler av en side kan ikke legges ved fra en nettleserøkt med Zero Data Retention.', en: 'Parts of a page cannot be attached from a browser session with Zero Data Retention.' },
  browser_attachment_region_unresolved: { no: 'Det valgte området kan ikke brukes ennå, fordi nettleservisningen ikke er klar.', en: 'The selected area cannot be used yet, because the browser view is not ready.' },
  browser_attachment_tab_unresolved: { no: 'Fanen kunne ikke knyttes til den åpne fanen som vises, så ingenting ble lagt ved.', en: 'The tab could not be matched to the open tab that is shown, so nothing was attached.' },
  browser_tab_context_refused_zdr: { no: 'Innhold fra en nettleserfane legges ikke ved i en økt med Zero Data Retention.', en: 'Content from a browser tab is not attached in a session with Zero Data Retention.' },
  browser_tab_mention_unresolved: { no: 'Fanen du nevnte, kunne ikke knyttes til nøyaktig én åpen fane, så ingenting ble lagt ved.', en: 'The tab you mentioned could not be matched to exactly one open tab, so nothing was attached.' },
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
  browser_attach_unavailable: {
    no: 'En tilkoblet agentnettleser er ikke tilgjengelig for denne økten.',
    en: 'An attached agent browser is not available for this session.',
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

  // Fetch failures Quarry names on a failed scrape (gateway `knowledge/shared.rs`, `fetch_sub_code_refusal`).
  // They arrive only with the fetch sub-codes switched on in quarry-edge. Other failures keep scrape_failed and scrape_timeout.
  scrape_host_not_found: {
    no: 'Vi fant ikke adressen til nettstedet. Sjekk lenken og prøv igjen.',
    en: 'We could not find the address of the site. Check the link and try again.',
  },
  scrape_unreachable: {
    no: 'Nettstedet svarer ikke akkurat nå.',
    en: 'The site is not responding right now.',
    retryable: true,
  },
  scrape_tls_failed: {
    no: 'Det var ikke mulig å opprette en sikker forbindelse til nettstedet. Prøv en annen kilde.',
    en: 'A secure connection to the site could not be made. Try another source.',
  },

  // Refusals the gateway answers before any upstream call: the address or the
  // request itself is wrong, so the person fixes it rather than retries.
  invalid_url: {
    no: 'Adressen ser ikke ut som en offentlig nettside. Sjekk lenken og prøv igjen.',
    en: 'The address does not look like a public web page. Check the link and try again.',
  },
  invalid_allowed_domains: {
    no: 'Listen over tillatte nettsteder er ikke gyldig. Sjekk domenene og prøv igjen.',
    en: 'The list of allowed sites is not valid. Check the domains and try again.',
  },
  browser_grant_invalid: {
    no: 'Nettlesertilgangen for denne kjøringen er ikke gyldig. Start kjøringen på nytt.',
    en: 'The browser access for this run is not valid. Start the run again.',
  },
  invalid_destination_id: {
    no: 'Mottakeren er ikke gyldig. Sjekk valget og prøv igjen.',
    en: 'The destination is not valid. Check the choice and try again.',
  },
  invalid_skill_decision: {
    no: 'Avgjørelsen kunne ikke leses. Velg godta eller avvis og prøv igjen.',
    en: 'The decision could not be read. Choose accept or reject and try again.',
  },
  scope_not_enabled: {
    no: 'Bare en avgjørelse for dette ene kallet er tilgjengelig.',
    en: 'Only a decision for this one call is available.',
  },
  rule_requires_thread_scope: {
    no: '«Alltid» kan bare brukes på en godkjenning for denne tråden.',
    en: 'An Always applies only to an approval for this thread.',
  },
  body_too_large: {
    no: 'Policyen er for stor. Kort den ned og prøv igjen.',
    en: 'The policy is too large. Shorten it and try again.',
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
  zdr_durable_mutation_forbidden: {
    no: 'Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.',
    en: 'Zero Data Retention credentials cannot create, change, or delete durable state.',
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

  // A local refusal, thrown by the chat client before anything is sent: the
  // ChatGPT-subscription route carries no images (shared/api/chat-client.ts).
  attachments_not_sent: { no: noStrings.attachments.notSent, en: enStrings.attachments.notSent },

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
  // The org model-access policy (gateway `orgs/model_entitlement.rs`, org-core
  // `modelentitlement`): the refusals of an admin save. A member's refusal to use a
  // model is a catalogue code, rendered below from its own module.
  revision_conflict: {
    no: 'Modelltilgangen ble endret av noen andre. Last inn på nytt og gjør endringen igjen.',
    en: 'Model access was changed by someone else. Reload and make the change again.',
  },
  invalid_model_policy: {
    no: 'Modelltilgangen er ikke gyldig. Sjekk modell-ID-ene, effektnivået og personvernnivået.',
    en: 'The model access policy is not valid. Check the model ids, the effort level and the privacy tier.',
  },
  model_policy_leaves_no_zdr_model: {
    no: 'Denne modelltilgangen ville latt en rolle stå uten modell med nullbevaringsgodkjenning, og Zero Data Retention er på. Tillat minst én godkjent modell.',
    en: 'This model access would leave a role with no zero-retention attested model while Zero Data Retention is on. Allow at least one attested model.',
  },
  model_policy_guard_unavailable: {
    no: 'Kunne ikke kontrollere modelltilgangen mot Zero Data Retention akkurat nå, så ingenting ble lagret. Prøv igjen om litt.',
    en: 'Model access could not be checked against Zero Data Retention right now, so nothing was saved. Try again shortly.',
  },
  zdr_models_not_entitled: {
    no: 'Zero Data Retention kan ikke slås på: modelltilgangen tillater ingen modell med nullbevaringsgodkjenning. Endre modelltilgangen først.',
    en: 'Zero Data Retention cannot be turned on: the model access allows no zero-retention attested model. Change the model access first.',
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
    no: 'Du har ikke tilgang til å lese samtalen i dette rommet. Organisasjonen kan ha slått av delt lesing, eller du trenger tilgang fra en eier eller leder av rommet.',
    en: 'You do not have access to read the conversation in this space. The organization may have turned off shared reading, or you need access from an owner or manager of the space.',
  },
  space_lifecycle_unavailable: {
    no: 'Rommet kunne ikke kontrolleres akkurat nå. Ingenting ble endret. Hvis du skrev en melding, er den beholdt; prøv igjen om litt.',
    en: 'The room could not be checked right now. Nothing was changed. If you were writing a message, it is kept; try again shortly.',
  },
  space_deletion_unavailable: {
    no: 'Slettingen av rommet kunne ikke kontrolleres akkurat nå. Prøv igjen om litt.',
    en: 'The deletion of this space could not be checked right now. Try again shortly.',
  },
  space_deletion_not_found: {
    no: 'Fant ingen slettingsforespørsel for dette rommet. Den kan allerede være fullført eller avbrutt.',
    en: 'No deletion request was found for this space. It may already be finished or cancelled.',
  },
  // EP-9 C4: a legal hold blocks a delete or an erasure until it is released in the
  // organization's settings; retrying changes nothing. An outage of the check is retryable.
  erasure_blocked_legal_hold: {
    no: 'En juridisk tilbakeholdelse gjelder disse dataene, så ingenting ble slettet. Samtalen og målene og versjonene i den beholdes til tilbakeholdelsen oppheves.',
    en: 'A legal hold covers this data, so nothing was deleted. The conversation and its goals and revisions are kept until the hold is released.',
  },
  erasure_hold_check_unavailable: {
    no: 'Juridiske tilbakeholdelser kunne ikke kontrolleres, så ingenting ble slettet.',
    en: 'Legal holds could not be checked, so nothing was deleted.',
    retryable: true,
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
    no: 'Fant ikke dette medlemmet. Personen er kanskje ikke medlem lenger; last inn på nytt.',
    en: 'This member was not found. The person may no longer be a member; reload.',
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
    no: 'Du er ikke lenger aktivt medlem, så du kan ikke gjøre dette. Ingenting ble sendt eller endret.',
    en: 'You are no longer an active member, so you cannot do this. Nothing was sent or changed.',
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
  agent_api_disabled: { no: 'Agent API er ikke slått på her ennå.', en: 'The Agent API is not turned on here yet.' },
  agent_api_feature_not_supported: { no: 'Denne funksjonen støttes ikke av Agent API ennå.', en: 'This feature is not supported by the Agent API yet.' },
  agent_api_scope_missing: { no: 'API-nøkkelen mangler tillatelsen som trengs for denne handlingen.', en: 'The API key lacks the permission this action needs.' },
  agent_builder_forbidden: { no: 'Bare en administrator i organisasjonen, den som laget agenten, eller en leder i rommet den ble laget i, kan endre den.', en: 'Only an organization admin, the agent creator, or a manager of the room it was made in can edit it.' },
  agent_definition_unavailable: { no: 'Agenten kunne ikke hentes akkurat nå. Prøv igjen om litt.', en: 'The agent could not be fetched right now. Try again shortly.' },
  agent_draft_missing: { no: 'Lagre et utkast før du publiserer eller forhåndsviser agenten.', en: 'Save a draft before you publish or preview the agent.' },
  agent_draft_unavailable: { no: 'Utkastet kunne ikke lagres. Ingenting ble endret. Prøv igjen om litt.', en: 'The draft could not be saved. Nothing was changed. Try again shortly.' },
  agent_not_found: { no: 'Fant ikke agenten, eller du har ikke tilgang til den.', en: 'The agent was not found, or you do not have access to it.' },
  agent_preview_not_in_thread: { no: 'En forhåndsvisning kjøres i en midlertidig chat, og den kan ikke knyttes til en samtale eller et rom.', en: 'A preview runs in a temporary chat, and it cannot be linked to a conversation or a room.' },
  agent_publish_unavailable: { no: 'Agenten kunne ikke publiseres, og den nåværende versjonen er uendret. Prøv igjen om litt.', en: 'The agent could not be published, and the current version is unchanged. Try again shortly.' },
  agent_run_invalid: { no: 'Kjøringen kunne ikke startes. Prøv igjen om litt.', en: 'The run could not be started. Try again shortly.' },
  agent_versions_unavailable: { no: 'Versjonshistorikken kunne ikke hentes akkurat nå. Prøv igjen om litt.', en: 'The version history could not be fetched right now. Try again shortly.' },
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
  invalid_agent_preview: { no: 'Forhåndsvisningen mangler agenten den skal vise.', en: 'The preview is missing the agent it should show.' },
  invalid_agent_ref: { no: 'Agenten er ikke gyldig. Gå tilbake og velg den på nytt.', en: 'The agent is not valid. Go back and choose it again.' },
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
    no: 'Fant ikke samtalen. Den kan være slettet. Hvis du skrev en melding, ble den ikke sendt.',
    en: 'The conversation was not found. It may have been deleted. If you were writing a message, it was not sent.',
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
  preview_unavailable: {
    no: 'Vi kunne ikke lese en trygg forhåndsvisning av nettsiden. Prøv på nytt, eller fortsett uten forhåndsvisning.',
    en: 'We could not read a safe preview of the website. Try again, or continue without a preview.',
  },
  no_org_number: {
    no: 'Organisasjonen har ikke organisasjonsnummer ennå. Legg det inn i organisasjonsinnstillingene, og prøv igjen.',
    en: 'The organization has no organisation number yet. Add it in the organization settings, and try again.',
  },
  invalid_checkout_request: {
    no: 'Betalingen kunne ikke startes med disse opplysningene. Sjekk dem og prøv igjen.',
    en: 'The payment could not be started with these details. Check them and try again.',
  },

  // Marketing, Visibility and Algolia setup (gateway `domains/marketing.rs`, `domains/integrations/algolia.rs`,
  // `domains/integrations/providers.rs`). The panels answer a 409 conflict by their own code; these are the refusals
  // the gateway sends before a request reaches the marketing core.
  algolia_unavailable: {
    no: 'Algolia-oppsettet er utilgjengelig. Sjekk tilgangene i arbeidsområdet og oppsettet hos leverandøren, og prøv igjen.',
    en: 'The Algolia setup is unavailable. Check the workspace permissions and the provider setup, and try again.',
  },
  invalid_provider: {
    no: 'Leverandøren er ikke gyldig. Velg en av leverandørene i listen.',
    en: 'The provider is not valid. Choose one of the providers in the list.',
  },
  control_session_unavailable: {
    no: 'Innloggingsøkten kunne ikke bekreftes akkurat nå. Prøv igjen om litt.',
    en: 'Your session could not be confirmed right now. Try again shortly.',
    retryable: true,
  },
  dns_verification_retired: {
    no: 'DNS-verifisering i Studio er avviklet. Koble til kontoen hos den aktuelle leverandøren i stedet.',
    en: 'DNS verification in Studio has been retired. Connect the account at the relevant provider instead.',
  },
  invalid_marketing_request: {
    no: 'Markedsføringsforespørselen er ikke gyldig. Velg en gyldig operasjon og et registrert nettsted, og prøv igjen.',
    en: 'The marketing request is not valid. Choose a valid operation and a registered website, and try again.',
  },
  invalid_task_assignee: {
    no: 'Den valgte personen er ikke et nåværende medlem av arbeidsområdet. Velg et nåværende medlem.',
    en: 'The chosen person is not a current member of the workspace. Choose a current member.',
  },
  marketing_admin_required: {
    no: 'Bare en administrator i arbeidsområdet kan styre integrasjoner. Be en administrator gjøre endringen.',
    en: 'Only a workspace administrator can manage integrations. Ask an administrator to make the change.',
  },
  marketing_json_required: {
    no: 'Forespørselen hadde feil format. Last siden på nytt og prøv igjen.',
    en: 'The request had the wrong format. Reload the page and try again.',
  },
  marketing_route_not_found: {
    no: 'Denne markedsføringsfunksjonen støttes ikke. Last siden på nytt, og kontakt support hvis det vedvarer.',
    en: 'This marketing function is not supported. Reload the page, and contact support if it persists.',
  },
  marketing_same_origin_required: {
    no: 'Forespørselen ble avvist fordi den ikke kom fra denne appen. Last siden på nytt og prøv igjen.',
    en: 'The request was refused because it did not come from this app. Reload the page and try again.',
  },
  marketing_scope_unavailable: {
    no: 'Markedsføring er ikke tilgjengelig i det aktive arbeidsområdet. Bytt arbeidsområde, eller spør en administrator.',
    en: 'Marketing is not available in the active workspace. Switch workspace, or ask an administrator.',
  },
  marketing_source_unavailable: {
    no: 'Markedsføringsdataene er midlertidig utilgjengelige. Prøv igjen om litt.',
    en: 'The marketing data is temporarily unavailable. Try again shortly.',
    retryable: true,
  },
  visibility_route_not_found: {
    no: 'Leadresultater hører til i Leads, ikke i Synlighet. Åpne dem der.',
    en: 'Lead outcomes belong in Leads, not in Visibility. Open them there.',
  },
  visibility_workspace_not_configured: {
    no: 'Synlighet er ikke satt opp for det aktive arbeidsområdet. Kontakt support.',
    en: 'Visibility is not set up for the active workspace. Contact support.',
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
  api_key_grant_invalid: { no: 'Nettlesertilgangen kunne ikke settes opp. Prøv igjen om litt.', en: 'Browser access could not be set up. Try again shortly.' },
  api_key_invalid: { no: 'API-nøkkelen mangler, er ukjent, deaktivert eller utløpt.', en: 'The API key is missing, unknown, disabled or expired.' },
  api_key_missing: { no: 'Du må oppgi en API-nøkkel.', en: 'You need to provide an API key.' },
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
    no: 'Forespørselen mangler noe eller er ugyldig. Ingenting er endret. Sjekk og prøv igjen.',
    en: 'The request is missing something or is not valid. Nothing was changed. Check it and try again.',
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
  // Tender matching: the core's answers and the gateway's own refusals.
  // (`rate_limited` is intentionally not repeated: the entry above is shared.)
  timeout: { no: 'Søket tok for lang tid. Prøv igjen.', en: 'The search took too long. Try again.' },
  not_configured: { no: 'Anbud er ikke koblet til i dette miljøet.', en: 'Tender search is not connected in this environment.' },
  tender_service_misconfigured: { no: 'Anbud-tjenesten er ikke riktig satt opp. Kontakt support.', en: 'The tender service is not set up correctly. Contact support.' },
  payload_too_large: { no: 'Profilen er for stor.', en: 'The profile is too large.' },
  validation_failed: { no: 'Sjekk profilen: noen felt er ikke gyldige.', en: 'Check the profile: some fields are not valid.' },
  bad_request: { no: 'Forespørselen er ikke gyldig.', en: 'The request is not valid.' },
  not_found: { no: 'Kunngjøringen finnes ikke lenger i lageret.', en: 'This notice is no longer in the store.' },
  unsupported_notice: { no: 'Formatet på kunngjøringen støttes ikke.', en: 'This notice format is not supported.' },
  unreadable_notice: { no: 'Kunngjøringen kunne ikke leses.', en: 'The notice could not be read.' },
  not_an_opportunity: { no: 'Dette er en tildeling, ikke et anbud du kan by på.', en: 'This is an award, not a tender you can bid on.' },

  // ---- Space room codes (P-SPA1) ----------------------------------------
  // Each Space workstream adds its codes ONLY under its own banner, keys sorted
  // within the banner, so two workstreams never edit the same lines. Copy for a
  // zero-retention or model-entitlement refusal does not go here: it comes from
  // the catalogue modules above.

  // WS01 human posts and threads
  idempotency_key_conflict: { no: 'Denne meldingen er allerede sendt med et annet innhold. Skriv den på nytt og send igjen.', en: 'This message was already sent with different text. Write it again and send.' },
  invalid_client_send_id: { no: 'Meldingen kunne ikke sendes på en trygg måte. Last inn på nytt og prøv igjen.', en: 'The message could not be sent safely. Reload and try again.' },
  invalid_post: { no: 'Meldingen er tom eller for lang. Ingenting ble sendt.', en: 'The message is empty or too long. Nothing was sent.' },
  managed_run_already_started: { no: 'Meldingen var allerede mottatt. Svaret vises i rommet.', en: 'This message was already received. The reply shows in the room.' },
  post_not_stored: { no: 'Samtalen ble opprettet, men meldingen ble ikke lagret. Prøv igjen; den sendes bare én gang.', en: 'The conversation was created but the message was not saved. Try again; it will only be sent once.' },
  session_unavailable: { no: 'Samtaletjenesten er utilgjengelig akkurat nå. Meldingen din er beholdt; prøv igjen om litt.', en: 'The conversation service is unavailable right now. Your message is kept; try again shortly.' },
  space_authority_stale: { no: 'Tilgangen din i rommet ble endret mens du skrev. Prøv igjen; hvis det fortsetter, last inn rommet på nytt.', en: 'Your access to the room changed while you were writing. Try again; if it keeps happening, reload the room.' },
  space_decision_unavailable: { no: 'Kunne ikke bekrefte at du kan skrive her akkurat nå. Meldingen din er beholdt; prøv igjen om litt.', en: 'We could not confirm that you may post here right now. Your message is kept; try again shortly.' },
  space_human_posts_disabled: { no: 'Å skrive til rommet uten agent er ikke slått på ennå. Bruk Spør Verevon.', en: 'Posting to the room without the agent is not turned on yet. Use Ask Verevon.' },
  space_post_not_authorized: { no: 'Rollen din i dette rommet tillater ikke å skrive her. Be en eier eller leder om redigeringstilgang.', en: 'Your role in this room does not allow posting here. Ask an owner or manager for edit access.' },
  thread_state_conflict: { no: 'Samtalen kan ikke ta imot nye meldinger akkurat nå. Prøv igjen om litt.', en: 'The conversation cannot take new messages right now. Try again shortly.' },

  // WS08 mentions and addressed wake
  agent_not_bound: { no: 'Denne agenten er ikke lagt til i rommet, så den ble ikke nevnt.', en: 'This agent has not been added to the room, so it was not mentioned.' },
  agent_respond_to_denied: { no: 'Denne agenten svarer ikke på meldinger fra deg i dette rommet. En leder bestemmer hvem som kan tilkalle den.', en: 'This agent does not answer messages from you in this room. A manager decides who can call it.' },
  invalid_mentions: { no: 'Meldingen har for mange eller ugyldige omtaler. Fjern noen og send på nytt.', en: 'The message has too many or invalid mentions. Remove some and send again.' },
  mention_text_missing: { no: 'Navnet står ikke lenger i meldingen, så omtalen ble ikke lagret.', en: 'The name is no longer in the message, so the mention was not recorded.' },
  mention_unavailable: { no: 'Omtaler kunne ikke lagres akkurat nå. Meldingen ble sendt som vanlig tekst.', en: 'Mentions could not be recorded right now. The message was sent as plain text.' },
  person_not_in_room: { no: 'Personen er ikke med i rommet, så omtalen ble ikke lagret.', en: 'That person is not in the room, so the mention was not recorded.' },
  // EP-6 S9 agent version pinning (BFF read and model gateway stream codes)
  agent_version_unavailable: { no: 'Agentens versjon kunne ikke leses akkurat nå. Prøv igjen snart.', en: 'This agent\'s version could not be read right now. Try again shortly.' },
  agent_pin_invalid: { no: 'Agentversjonen for denne meldingen kunne ikke leses.', en: 'The agent version for this turn could not be read.' },
  agent_tool_allowlist_do_unsupported: { no: 'Denne agentens verktøyliste kan ikke håndheves i en Gjør-kjøring ennå. Spør den i stedet.', en: 'This agent\'s tool list cannot be enforced on a Do run yet. Ask it instead.' },
  // EP-6 S2 Run now: the BFF refuses a schedule someone else created
  run_now_not_creator: { no: 'Bare den som opprettet planen kan kjøre den nå.', en: 'Only the person who created this schedule can run it now.' },
  space_addressed_wake_disabled: { no: 'Å tilkalle agenter fra en melding er ikke slått på ennå.', en: 'Calling agents from a message is not turned on yet.' },
  wake_not_authorized: { no: 'Du kan ikke tilkalle denne agenten her. Meldingen er sendt, men agenten svarer ikke.', en: 'You cannot call this agent here. Your message was sent, but the agent will not answer.' },
  wake_owner_required: { no: 'Bare den som startet denne samtalen kan tilkalle en agent i den. Meldingen er sendt uten svar fra agenten.', en: 'Only the person who started this conversation can call an agent in it. Your message was sent without an answer from the agent.' },
  wake_requires_single_agent: { no: 'Én agent kan tilkalles om gangen. Prøv igjen.', en: 'One agent can be called at a time. Try again.' },
  wake_requires_thread: { no: 'Agenten kan bare tilkalles fra en melding som er lagret i en samtale.', en: 'An agent can only be called from a message stored in a conversation.' },
  wake_trigger_mismatch: { no: 'Meldingen er endret siden den ble sendt, så agenten ble ikke tilkalt.', en: 'The message changed after it was sent, so the agent was not called.' },
  // WS08-S8 wake policy editor
  agent_binding_not_found: { no: 'Agenten er ikke lenger knyttet til dette rommet. Ingenting er endret.', en: 'The agent is no longer bound to this room. Nothing was changed.' },
  invalid_policy: { no: 'Valget er ikke gyldig. Velg minst én person for en liste, og høyst 50. Ingenting er endret.', en: 'That choice is not valid. A list needs at least one person and at most 50. Nothing was changed.' },
  policy_conflict: { no: 'Noen andre har endret hvem som kan tilkalle agenten. Den nyeste innstillingen er lastet inn; se over og lagre på nytt.', en: 'Someone else changed who can wake the agent. The latest setting is loaded; review it and save again.' },
  space_role_cannot_manage_agent: { no: 'Bare en eier eller leder kan endre hvem som kan tilkalle agenten.', en: 'Only an owner or manager can change who can wake the agent.' },

  // WS02 realtime stream
  space_stream_capacity: { no: 'Live-oppdateringer er midlertidig full. Rommet oppdateres hvert par sekunder.', en: 'Live updates are temporarily at capacity. The room is refreshing every few seconds.' },
  space_stream_disabled: { no: 'Live-oppdateringer er ikke slått på. Rommet oppdateres hvert par sekunder.', en: 'Live updates are not turned on. The room is refreshing every few seconds.' },
  space_stream_forbidden: { no: 'Live-oppdateringer er ikke tilgjengelige for deg i dette rommet.', en: 'Live updates are not available to you in this room.' },
  space_stream_unavailable: { no: 'Live-oppdateringer er midlertidig utilgjengelige. Rommet oppdateres hvert par sekunder.', en: 'Live updates are temporarily unavailable. The room is refreshing every few seconds.' },

  // WS03 policy and denials
  invalid_memory: { no: 'Minnet er ikke gyldig, så ingenting ble endret. Last inn siden på nytt.', en: 'The memory is not valid, so nothing was changed. Reload the page.' },
  memory_not_found: { no: 'Fant ikke minnet, eller du har ikke tilgang til det. Ingenting ble endret.', en: 'The memory was not found, or you do not have access to it. Nothing was changed.' },
  space_memory_not_authorized: { no: 'Du har ikke tilgang til å administrere minnet i dette rommet.', en: 'You do not have access to manage the memory in this room.' },
  space_memory_unavailable: { no: 'Minnet i rommet er midlertidig utilgjengelig. Prøv igjen om litt.', en: 'The memory in this room is temporarily unavailable. Try again shortly.' },

  // WS14 document sharing (documents-api share routes)
  shares_disabled: { no: 'Deling er ikke slått på ennå.', en: 'Sharing is not turned on yet.' },
  sharing_unavailable: { no: 'Deling er midlertidig utilgjengelig. Ingenting er endret. Prøv igjen om litt.', en: 'Sharing is temporarily unavailable. Nothing was changed. Try again shortly.' },
  role_not_supported: { no: 'Dokumenter kan bare deles for visning.', en: 'Documents can only be shared for viewing.' },
  self_share: { no: 'Du eier dokumentet allerede.', en: 'You already own this document.' },
  share_limit_reached: { no: 'Dokumentet er delt med så mange som er tillatt. Fjern noen først.', en: 'This document is already shared with as many people as allowed. Remove someone first.' },

  // WS14 room files (S9; gateway spaces/files.rs and imports-core refusals)
  file_not_found: { no: 'Fant ikke filen, eller du har ikke tilgang til den.', en: 'The file was not found, or you do not have access to it.' },
  room_file_count_exceeded: { no: 'For mange filer på én gang. Velg færre og prøv igjen.', en: 'Too many files at once. Pick fewer and try again.' },
  room_file_duplicate: { no: 'Denne filen ligger allerede i rommet.', en: 'This file is already in the room.' },
  room_file_empty: { no: 'Filen er tom.', en: 'The file is empty.' },
  room_file_no_extractable_text: { no: 'Fant ingen tekst i filen. Skannede dokumenter støttes ikke ennå.', en: 'No text could be read from the file. Scanned documents are not supported yet.' },
  room_file_text_too_long: { no: 'Teksten i filen er for lang til å lagres i rommet.', en: 'The text in the file is too long to keep in the room.' },
  room_file_too_large: { no: 'Filen er for stor til å lagres i rommet.', en: 'The file is too large to keep in the room.' },
  room_file_type_unsupported: { no: 'Denne filtypen kan ikke lagres i rommet.', en: 'This file type cannot be kept in the room.' },
  room_files_disabled: { no: 'Romfiler er ikke slått på ennå.', en: 'Room files are not turned on yet.' },
  room_files_quota_exceeded: { no: 'Rommet har nådd grensen for lagrede filer. Fjern noen først.', en: 'The room has reached its limit of kept files. Remove some first.' },
  room_files_unavailable: { no: 'Romfiler er utilgjengelige akkurat nå. Ingenting er endret. Prøv igjen om litt.', en: 'Room files are unavailable right now. Nothing was changed. Try again shortly.' },
  space_files_not_authorized: { no: 'Rollen din i dette rommet tillater ikke å legge til filer.', en: 'Your role in this room does not allow adding files.' },
  upload_not_found: { no: 'Fant ikke opplastingen.', en: 'The upload was not found.' },

  // WS04 membership and room administration
  directory_unavailable: { no: 'Katalogen er utilgjengelig akkurat nå. Prøv igjen om litt.', en: 'The directory is unavailable right now. Try again shortly.' },
  // WS04-S8 room directory, visibility and self-join
  directory_disabled: { no: 'Organisasjonen din har ikke slått på romkatalogen, så rom kan ikke åpnes for alle ennå. Spør en administrator.', en: 'Your organization has not turned on the room directory, so rooms cannot be opened to everyone yet. Ask an administrator.' },
  idempotency_key_reused: { no: 'Denne forespørselen er allerede brukt til noe annet. Last inn på nytt og prøv igjen.', en: 'This request was already used for something else. Reload and try again.' },
  member_not_in_organization: { no: 'Personen er ikke med i organisasjonen, så de kan ikke legges til i rommet.', en: 'That person is not in the organization, so they cannot be added to the room.' },
  room_archived: { no: 'Rommet er arkivert. Medlemmer kan ikke endres.', en: 'The room is archived. Members cannot be changed.' },
  room_delegation_required: { no: 'Du har ikke tilgang til å endre medlemmer her.', en: 'You do not have access to change members here.' },
  room_details_invalid: { no: 'Navnet eller emnet er ikke gyldig. Ingenting er endret.', en: 'The room name or topic is not valid. Nothing was changed.' },
  room_details_unavailable: { no: 'Navnet og emnet kunne ikke lagres. Ingenting er endret. Prøv igjen om litt.', en: 'The name and topic could not be saved. Nothing was changed. Try again shortly.' },
  room_not_active: { no: 'Rommet er ikke aktivt, så navn og emne kan ikke endres.', en: 'The room is not active, so its name and topic cannot be changed.' },
  space_kind_not_editable: { no: 'Navn og emne kan bare endres på et vanlig rom, ikke på et personlig rom.', en: 'The name and topic can only be changed on a regular room, not a personal Space.' },
  room_elevated_role_requires_owner: { no: 'Bare en eier kan gi, endre eller fjerne rollen leder eller eier.', en: 'Only an owner can grant, change or remove a manager or owner.' },
  room_member_changed: { no: 'Medlemmet ble endret av noen andre. Listen er lastet på nytt; prøv igjen.', en: 'That member was changed by someone else. The list was reloaded; try again.' },
  room_not_found: { no: 'Fant ikke rommet, eller du har ikke tilgang til det.', en: 'The room was not found, or you do not have access to it.' },
  room_personal_space: { no: 'Et personlig rom har bare én eier og kan ikke ha flere medlemmer.', en: 'A personal Space has one owner and cannot have other members.' },
  room_role_forbidden: { no: 'Rollen din i dette rommet tillater ikke dette.', en: 'Your role in this room does not allow that.' },
  room_roster_derived: { no: 'Medlemmene i dette rommet følger organisasjonen og redigeres ikke her.', en: 'The members of this room follow the organization and are not edited here.' },
  room_sole_owner: { no: 'Rommet må ha minst én eier. Gjør noen andre til eier først.', en: 'The room needs at least one owner. Make someone else an owner first.' },
  // WS12 direct messages
  dm_disabled: { no: 'Direktemeldinger er ikke slått på.', en: 'Direct messages are not turned on.' },
  dm_not_entitled: { no: 'Organisasjonen din har ikke direktemeldinger slått på. Spør en administrator.', en: 'Your organization has not turned on direct messages. Ask an administrator.' },
  dm_participants_invalid: { no: 'Disse personene kan ikke være med i samme melding. Velg én til åtte andre i organisasjonen.', en: 'These people cannot be in one message. Pick one to eight others in the organization.' },
  dm_registration_failed: { no: 'En melding med disse personene kunne ikke settes opp. Be en administrator rydde opp.', en: 'A message with these people could not be set up. Ask an administrator to clear it.' },
  dm_hide_unavailable: { no: 'Meldingen kunne ikke oppdateres. Ingenting er endret. Prøv igjen om litt.', en: 'The message could not be updated. Nothing was changed. Try again shortly.' },
  dm_agent_bind_refused: { no: 'En direktemelding kan ikke ha agenter.', en: 'A direct message cannot have agents.' },
  space_not_hideable: { no: 'Bare en direktemelding kan skjules.', en: 'Only a direct message can be hidden.' },
  space_authority_unavailable: { no: 'Kunne ikke bekrefte tilgangen akkurat nå. Ingenting er endret. Prøv igjen om litt.', en: 'Access could not be confirmed right now. Nothing was changed. Try again shortly.' },

  // WS12-S7 message edit and delete
  space_message_revise_disabled: { no: 'Redigering og sletting av meldinger er ikke slått på.', en: 'Editing and deleting messages is not turned on.' },
  space_message_revise_not_authorized: { no: 'Organisasjonen din tillater ikke at meldinger endres eller slettes. Spør en administrator.', en: 'Your organization does not allow messages to be changed or deleted. Ask an administrator.' },
  message_revise_forbidden: { no: 'Du kan bare endre dine egne meldinger, svar på dine egne kjøringer, eller som leder fjerne andres der organisasjonen tillater det.', en: 'You can only change your own messages, answers to your own runs, or, as a manager where your organization allows it, remove other people\'s.' },
  message_not_found: { no: 'Fant ikke meldingen. Den kan være slettet.', en: 'The message was not found. It may have been deleted.' },
  message_already_deleted: { no: 'Meldingen er allerede slettet.', en: 'The message was already deleted.' },
  revision_noop: { no: 'Meldingen er uendret. Ingenting ble lagret.', en: 'The message is unchanged. Nothing was saved.' },
  revision_limit: { no: 'Meldingen er endret for mange ganger.', en: 'The message has been changed too many times.' },
  message_state_conflict: { no: 'Meldingen ble endret av noen andre mens du jobbet. Last inn på nytt og prøv igjen.', en: 'The message was changed by someone else while you worked. Reload and try again.' },
  invalid_message: { no: 'Meldingen er tom eller for lang, eller valget er ugyldig. Ingenting er endret.', en: 'The message is empty or too long, or the choice is not valid. Nothing was changed.' },

  // WS12-S8 reactions
  space_reactions_disabled: { no: 'Reaksjoner er ikke slått på.', en: 'Reactions are not turned on.' },
  space_role_cannot_react: { no: 'Du har lesetilgang i dette rommet og kan ikke reagere.', en: 'You have read-only access in this room, so you cannot react.' },
  invalid_emoji: { no: 'Velg én emoji.', en: 'Pick a single emoji.' },
  invalid_reaction: { no: 'Reaksjonen er ikke gyldig. Ingenting er endret.', en: 'The reaction is not valid. Nothing was changed.' },
  reaction_limit: { no: 'Meldingen har allerede så mange ulike reaksjoner som den kan ha.', en: 'The message already has as many different reactions as it can hold.' },
  application_unavailable: { no: 'Reaksjonen kunne ikke lagres akkurat nå. Ingenting er endret. Prøv igjen om litt.', en: 'The reaction could not be saved right now. Nothing was changed. Try again shortly.' },

  // WS05 org security

  // WS13 message search
  entry_not_found: { no: 'Denne meldingen er ikke tilgjengelig for deg.', en: 'This message is not available to you.' },
  invalid_cursor: { no: 'Søkesiden er ikke gyldig lenger. Søk på nytt.', en: 'That page of results is no longer valid. Search again.' },
  invalid_filter: { no: 'Søkeavgrensningen er ikke gyldig. Velg omfanget på nytt og prøv igjen.', en: 'The search filter is not valid. Choose the scope again and try again.' },
  search_busy: { no: 'Søket er opptatt akkurat nå. Prøv igjen om litt.', en: 'Search is busy right now. Try again shortly.', retryable: true },
  search_timeout: { no: 'Søket tok for lang tid. Prøv igjen, eller gjør søket mer presist.', en: 'The search took too long. Try again, or make the search more specific.', retryable: true },
  search_unavailable: { no: 'Søket er utilgjengelig akkurat nå. Prøv igjen om litt.', en: 'Search is unavailable right now. Try again shortly.', retryable: true },
  space_search_disabled: { no: 'Meldingssøk er ikke slått på for dette rommet.', en: 'Message search is not turned on for this room.' },

  // WS07 resilience and content hardening

  // WS10 tool policy editor (S5b)
  tool_policy_role_required: { no: 'Bare en eier eller leder kan endre verktøyreglene.', en: 'Only an owner or manager can change the tool rules.' },
  tool_policy_invalid: { no: 'Verktøyreglene er ikke gyldige. Ingenting er endret.', en: 'The tool rules are not valid. Nothing was changed.' },
  unsupported_policy_field: { no: 'Forespørselen inneholdt noe som ikke kan settes her. Ingenting er endret.', en: 'The request contained something that cannot be set here. Nothing was changed.' },
  tool_policy_revision_conflict: { no: 'Reglene ble endret av noen andre. De nyeste er lastet inn; se over og lagre på nytt.', en: 'Someone else changed the rules. The latest are loaded; review them and save again.' },
  tool_policy_unavailable: { no: 'Verktøyreglene er utilgjengelige akkurat nå. Ingenting er endret. Prøv igjen om litt.', en: 'The tool rules are unavailable right now. Nothing was changed. Try again shortly.' },
  approval_agent_not_in_space: { no: 'Agenten deltar ikke lenger i rommet, så den kan ikke få egne verktøyregler.', en: 'The agent is no longer part of the room, so it cannot have rules of its own.' },
  // WS11 attention, notify level, mark unread
  space_attention_disabled: { no: 'Varsler på tvers av rom er ikke slått på.', en: 'Cross-room alerts are not turned on.' },
  invalid_level: { no: 'Det varselnivået finnes ikke. Ingenting er endret.', en: 'That notification level does not exist. Nothing was changed.' },
  space_index_unavailable: { no: 'Rommene dine kunne ikke leses akkurat nå. Prøv igjen om litt.', en: 'Your rooms could not be read right now. Try again shortly.', retryable: true },
  attention_item_not_found: { no: 'Det elementet finnes ikke lenger i innboksen din.', en: 'That item is no longer in your inbox.' },
  invalid_snooze: { no: 'Utsett i minst ett minutt og høyst 30 dager. Ingenting er endret.', en: 'Snooze for at least a minute and at most 30 days. Nothing was changed.' },
  attention_unavailable: { no: 'Innboksen er ikke tilgjengelig akkurat nå. Ingenting er endret.', en: 'The inbox is not available right now. Nothing was changed.', retryable: true },
  invalid_follow_state: { no: 'Det valget finnes ikke. Ingenting er endret.', en: 'That choice does not exist. Nothing was changed.' },
  space_kind_unsupported: { no: 'Et personlig rom har ingen delte agenter og dermed ingen verktøyregler.', en: 'A personal Space has no shared agents, so it has no tool rules.' },
  // WS15 room timeline windows (S7/S8)
  space_timeline_unavailable: { no: 'Samtalene i rommet kunne ikke leses akkurat nå. Prøv igjen om litt.', en: 'The room conversations could not be read right now. Try again shortly.', retryable: true },
  space_timeline_disabled: { no: 'Sideinndelte samtaler er ikke slått på for dette rommet.', en: 'Paged conversations are not turned on for this room.' },
  summary_backfill_pending: { no: 'Rommet gjøres klart for sideinndeling. Prøv igjen om litt.', en: 'The room is being prepared for paging. Try again shortly.', retryable: true },
  invalid_window: { no: 'Den delen av samtalen kunne ikke hentes. Last siden på nytt.', en: 'That part of the conversation could not be loaded. Reload the page.' },
  invalid_refresh: { no: 'Oppdateringen kunne ikke gjennomføres. Last siden på nytt.', en: 'The refresh could not be completed. Reload the page.' },
  approval_already_decided: { no: 'Denne godkjenningen er allerede avgjort, sannsynligvis av noen andre.', en: 'This approval was already decided, most likely by someone else.' },
  approval_authority_unavailable: { no: 'Godkjenningstjenesten er utilgjengelig akkurat nå. Ingenting er endret.', en: 'The approval service is unavailable right now. Nothing was changed.', retryable: true },
  approval_expired: { no: 'Godkjenningen har utløpt.', en: 'This approval has expired.' },
  approval_not_addressed_to_you: { no: 'Denne godkjenningen venter på medlemmet som startet kjøringen.', en: 'This approval is waiting for the member who started the run.' },
  approval_not_found: { no: 'Fant ikke godkjenningen.', en: 'The approval was not found.' },
  approval_scope_not_permitted: { no: 'Dette omfanget av godkjenning er ikke tilgjengelig her.', en: 'This approval scope is not available here.' },
  feature_unavailable: { no: 'Denne funksjonen er ikke slått på for deg ennå.', en: 'This feature is not turned on for you yet.' },
  files_binding_unavailable: { no: 'Rommet er ikke koblet til en kunnskapskilde for filer.', en: 'This room is not connected to a knowledge target for files.' },
  files_read_not_authorized: { no: 'Du har ikke tilgang til å lese filene i dette rommet.', en: 'You are not allowed to read the files of this room.' },
  files_upstream_unavailable: { no: 'Filene i rommet kunne ikke hentes akkurat nå. Prøv igjen om litt.', en: 'The files of this room could not be loaded right now. Try again shortly.', retryable: true },
  idempotency_key_invalid: { no: 'Forespørselen mangler en gyldig nøkkel. Last inn på nytt og prøv igjen.', en: 'The request is missing a valid key. Reload and try again.' },
  identity_admin_required: { no: 'Identitetsadministrasjon krever at du er eier eller administrator i organisasjonen.', en: 'Identity administration requires an organization owner or admin role.' },
  invalid_before_ms: { no: 'Tidspunktet for oppslaget er ugyldig. Last inn på nytt.', en: 'The lookup time is not valid. Reload the page.' },
  invalid_document: { no: 'Dokumentet mangler en id.', en: 'A document id is required.' },
  invalid_domain: { no: 'Domenet er ikke gyldig.', en: 'The domain is not valid.' },
  invalid_kind: { no: 'Den typen varsel finnes ikke.', en: 'That kind of attention item does not exist.' },
  invalid_limit: { no: 'Antallet må være mellom 1 og 200.', en: 'The limit must be between 1 and 200.' },
  invalid_max_chars: { no: 'Tegngrensen må være et positivt tall.', en: 'The character limit must be a positive number.' },
  invalid_reason: { no: 'Årsaken til lesingen er ikke gyldig. Last inn på nytt.', en: 'The reason for reading is not valid. Reload the page.' },
  invalid_refs: { no: 'Oppgi romreferanser adskilt med komma, høyst 60 om gangen.', en: 'Give Space references separated by commas, at most 60 at once.' },
  invalid_reminder_time: { no: 'Påminnelsen må settes mellom fem sekunder og 366 dager fram i tid.', en: 'The reminder must be set between five seconds and 366 days from now.' },
  invalid_run: { no: 'Kjøringen mangler en gyldig id.', en: 'A valid run id is required.' },
  invalid_sidebar_doc: { no: 'Sidepanelets oppsett er ikke gyldig. Ingenting er endret.', en: 'The sidebar layout is not valid. Nothing was changed.' },
  invalid_state: { no: 'Tilstanden må være åpen, utsatt eller ferdig.', en: 'The state must be open, snoozed or done.' },
  invalid_status: { no: 'Statusen må være tilgjengelig, borte, forstyrr ikke eller frakoblet, med høyst 80 tegn og utløp innen 7 dager.', en: 'The status must be available, away, do not disturb or offline, with at most 80 characters and an expiry within 7 days.' },
  invalid_thread: { no: 'Samtalen mangler en id.', en: 'A thread id is required.' },
  invalid_upload: { no: 'Opplastingen er tom, eller den kunne ikke godtas.', en: 'The upload is empty, or it could not be accepted.' },
  invalid_wake: { no: 'Vekkingen er ikke gyldig.', en: 'The wake request is not valid.' },
  pins_unavailable: { no: 'Festede innlegg kunne ikke hentes akkurat nå. Prøv igjen om litt.', en: 'The pinned posts could not be loaded right now. Try again shortly.', retryable: true },
  reminder_not_found: { no: 'Fant ikke påminnelsen.', en: 'That reminder was not found.' },
  reminders_unavailable: { no: 'Påminnelsene er utilgjengelige akkurat nå. Ingenting er endret.', en: 'Reminders are unavailable right now. Nothing was changed.', retryable: true },
  room_notices_unavailable: { no: 'Endringene for agenter og rom kunne ikke hentes akkurat nå. Prøv igjen om litt.', en: 'The agent and room changes could not be loaded right now. Try again shortly.', retryable: true },
  room_pins_limit: { no: 'Rommet har nådd grensen for festede innlegg. Løsne ett innlegg først.', en: 'This room has reached its pin limit. Unpin a post first.' },
  room_pins_unavailable: { no: 'Festingen kunne ikke endres. Ingenting er endret.', en: 'The pin could not be changed. Nothing was changed.', retryable: true },
  run_control_unavailable: { no: 'Kontrollen av kjøringen er midlertidig utilgjengelig.', en: 'Run control is temporarily unavailable.', retryable: true },
  run_not_found: { no: 'Fant ikke kjøringen, eller du har ikke tilgang til den.', en: 'The run was not found, or you do not have access to it.' },
  sidebar_too_large: { no: 'Sidepanelets oppsett er for stort til å lagres.', en: 'The sidebar layout is too large to save.' },
  space_reminders_disabled: { no: 'Påminnelser i rom er ikke slått på.', en: 'Space reminders are not turned on.' },
  space_run_control_not_authorized: { no: 'Du kan ikke stoppe denne kjøringen.', en: 'Stopping this run is not authorized.' },
  space_sidebar_conflict: { no: 'Sidepanelets oppsett er endret et annet sted. Last det inn på nytt og gjør endringen igjen.', en: 'The sidebar layout changed somewhere else. Reload it and apply your change again.' },
  space_sidebar_disabled: { no: 'Sidepanelet for rom er ikke slått på.', en: 'The Spaces sidebar layout is not turned on.' },
  space_sidebar_unavailable: { no: 'Sidepanelets oppsett er utilgjengelig akkurat nå. Ingenting er endret.', en: 'The sidebar layout is unavailable right now. Nothing was changed.', retryable: true },
  space_status_disabled: { no: 'Status i rom er ikke slått på.', en: 'Space status is not turned on.' },
  sso_policy_unavailable: { no: 'Innloggingspolicyen kunne ikke bekreftes. Prøv igjen om litt.', en: 'The sign-in policy could not be verified. Try again shortly.', retryable: true },
  sso_required: { no: 'Organisasjonen din krever innlogging med enkel pålogging (SSO).', en: 'Your organization requires single sign-on.' },
  status_unavailable: { no: 'Statusen er utilgjengelig akkurat nå. Ingenting er endret.', en: 'Status is unavailable right now. Nothing was changed.', retryable: true },
  system_entries_unavailable: { no: 'Endringene i medlemskap kunne ikke hentes fra kontrolltjenesten akkurat nå.', en: 'The membership changes could not be loaded from Control right now.', retryable: true },
  too_many_reminders: { no: 'Du har flest påminnelser som er tillatt. Avbryt én for å legge til en ny.', en: 'You have the most pending reminders allowed. Cancel one to add another.' },
  upload_length_required: { no: 'Opplastingen må oppgi lengden sin.', en: 'The upload must declare its length.' },
  zdr_feature_not_supported: { no: 'Dette støttes ikke med null datalagring ennå: agentkjøringer. Forespørselen ble avvist før noe kall til modellen.', en: 'This is not supported with zero data retention yet: agent runs. The request was refused before any model call.' },
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

/**
 * Why a memory save was refused in a shared Space or an agent run, keyed by the
 * `details.reason` of a `memory_not_enabled` refusal. The person's own pause or
 * off keeps the generic copy: those are theirs to switch back on, these are not.
 */
const MEMORY_SCOPE_COPY: ReadonlyMap<string, ErrorCopy> = new Map<string, ErrorCopy>([
  [
    'space_scope',
    {
      no: 'Personlig minne brukes og lagres ikke i delte Spaces, så dette ble ikke lagret.',
      en: 'Personal memory is not used or saved in shared Spaces, so this was not saved.',
    },
  ],
  [
    'agent_run_scope',
    {
      no: 'Minne lagres ikke fra en agentkjøring, så dette ble ikke lagret.',
      en: 'Memory is not saved from an agent run, so this was not saved.',
    },
  ],
])

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

  // A zero-retention refusal of the Model Plane catalogue: its own words, with
  // the feature it names (an ApiError carries it) and the next step. A message
  // the SPA wrote itself (LocalizedError) still wins.
  if (code && isZdrRefusalCode(code) && !(err instanceof LocalizedError)) {
    const shown = zdrRefusalPresentation(code, typeof like?.feature === 'string' ? like.feature : null, tr)
    return {
      code,
      kind,
      message: shown.next === null ? shown.message : `${shown.message} ${shown.next}`,
      resumable: like?.resumable === true,
      retryAfterSeconds,
    }
  }
  // A model-entitlement refusal of the Model Plane catalogue: its own words and
  // the next step. The reason and model refine it only where a caller has them.
  if (code && isModelEntitlementCode(code) && !(err instanceof LocalizedError)) {
    const shown = modelEntitlementPresentation(code, tr)
    return {
      code,
      kind,
      message: shown.next === null ? shown.message : `${shown.message} ${shown.next}`,
      resumable: like?.resumable === true,
      retryAfterSeconds,
    }
  }

  // A typed Space denial (WS03): the reason, not the operation code, says why. A
  // message the SPA wrote itself (LocalizedError) still wins.
  // A memory refusal is Session Core's, not Control's: its `details.reason` (`space_scope`,
  // `agent_run_scope`) shares a token with the Space denial vocabulary, so it is read below.
  if (isSpaceDenial(err) && !(err instanceof LocalizedError) && code !== 'memory_not_enabled') {
    return {
      code,
      kind,
      message: presentSpaceDenialText(err, tr),
      resumable: like?.resumable === true,
      retryAfterSeconds,
    }
  }

  let copy: ErrorCopy | undefined
  if (err instanceof LocalizedError) copy = err.copy
  // A refusal that names why the wall is up gets the sentence for that reason.
  if (kind === 'blocked' && typeof like?.blockReason === 'string') copy = BLOCK_REASON_COPY[like.blockReason]
  // A memory refusal that names a shared Space or an agent run says so.
  const memoryScope = like?.details?.reason
  if (code === 'memory_not_enabled' && typeof memoryScope === 'string') copy ??= MEMORY_SCOPE_COPY.get(memoryScope)
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
