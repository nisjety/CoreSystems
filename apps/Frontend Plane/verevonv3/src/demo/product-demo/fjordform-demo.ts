/**
 * The demo build's copy of the film fixtures (src/demo/fixtures/fjordform.ts).
 *
 * src/demo/install.ts, which the films and the demo share, imports the film's approval `nordlysApproval` from
 * './fixtures/fjordform'. vite.demo.config.ts maps that one import to this module, so the demo bundle does not carry
 * the film's retired case words. Every other name is re-exported unchanged. The films are built without this alias
 * and keep their own fixture.
 */
export * from '../fixtures/fjordform'

/**
 * The approval the approvals route answers for a run the live engine does not know. The live engine answers its own
 * runs first (src/demo/live/routes.ts), so a visitor does not see this object in practice.
 */
export const nordlysApproval = {
  id: 'apr_demo_oppfolging',
  run_id: 'run_demo_oppfolging',
  kind: 'Forbered oppfølging',
  status: 'APPROVAL_STATE_REQUESTED',
  detail: 'Tre steg er klare. Ingenting sendes før du godkjenner.',
}
