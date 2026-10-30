/** Keep fictional scene fixtures off ordinary app routes and production builds. */
export function shouldEnableSceneMode(
  flag: string | undefined,
  isDevelopment: boolean,
  pathname: string,
): boolean {
  return flag === '1' && isDevelopment && /^\/scene\/[a-z0-9][a-z0-9-]*\/?$/.test(pathname)
}
