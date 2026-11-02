/**
 * The build this tab is running (WS07-S2). `__VEREVON_BUILD_ID__` is compiled in
 * by vite.config.ts from the image's SOURCE_REVISION; it is `dev` outside an
 * image build, and the stale-tab check never compares a `dev` build.
 */
export const BUILD_ID: string =
  typeof __VEREVON_BUILD_ID__ === 'string' && __VEREVON_BUILD_ID__ !== '' ? __VEREVON_BUILD_ID__ : 'dev'

/** True for a local/dev build, which has no deploy to be stale against. */
export const isDevBuild = (): boolean => BUILD_ID === 'dev'
