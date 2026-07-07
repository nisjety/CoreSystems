export type MetaFacebookSdkConfig = {
  enabled?: boolean
  appId?: string
  apiVersion?: string
  locale?: string
  loginConfigId?: string
}

type FacebookLoginStatus = 'connected' | 'not_authorized' | 'unknown'

type FacebookAuthResponse = {
  accessToken?: string
  expiresIn?: number | string
  signedRequest?: string
  userID?: string
}

export type FacebookLoginResponse = {
  status?: FacebookLoginStatus
  authResponse?: FacebookAuthResponse
}

type FacebookSDK = {
  init: (options: {
    appId: string
    cookie: boolean
    xfbml: boolean
    version: string
  }) => void
  getLoginStatus: (callback: (response: FacebookLoginResponse) => void) => void
  login: (
    callback: (response: FacebookLoginResponse) => void,
    options?: Record<string, string | boolean>,
  ) => void
  AppEvents?: {
    logPageView?: () => void
  }
}

declare global {
  interface Window {
    FB?: FacebookSDK
    fbAsyncInit?: () => void
  }
}

let sdkPromise: Promise<FacebookSDK> | null = null
let sdkConfigKey = ''

export function isMetaFacebookSdkEnabled(config: MetaFacebookSdkConfig | undefined): config is MetaFacebookSdkConfig {
  return Boolean(config?.enabled && config.appId?.trim())
}

export function loadMetaFacebookSdk(config: MetaFacebookSdkConfig): Promise<FacebookSDK> {
  if (!isMetaFacebookSdkEnabled(config)) {
    return Promise.reject(new Error('Meta JavaScript SDK is not configured.'))
  }

  const normalized = normalizeConfig(config)
  const nextConfigKey = `${normalized.appId}:${normalized.apiVersion}:${normalized.locale}`
  if (sdkPromise && sdkConfigKey === nextConfigKey) return sdkPromise

  sdkConfigKey = nextConfigKey
  sdkPromise = new Promise<FacebookSDK>((resolve, reject) => {
    if (window.FB) {
      initializeFacebookSdk(window.FB, normalized)
      resolve(window.FB)
      return
    }

    window.fbAsyncInit = () => {
      if (!window.FB) {
        reject(new Error('Meta JavaScript SDK did not expose FB.'))
        return
      }
      initializeFacebookSdk(window.FB, normalized)
      resolve(window.FB)
    }

    const existing = document.getElementById('facebook-jssdk') as HTMLScriptElement | null
    if (existing) return

    const firstScript = document.getElementsByTagName('script')[0]
    const script = document.createElement('script')
    script.id = 'facebook-jssdk'
    script.async = true
    script.defer = true
    script.src = `https://connect.facebook.net/${normalized.locale}/sdk.js`
    script.onerror = () => reject(new Error('Meta JavaScript SDK could not be loaded.'))
    if (firstScript?.parentNode) {
      firstScript.parentNode.insertBefore(script, firstScript)
    } else {
      const parent = document.head || document.body || document.documentElement
      parent.appendChild(script)
    }
  }).catch((error) => {
    sdkPromise = null
    sdkConfigKey = ''
    throw error
  })

  return sdkPromise
}

export async function getMetaLoginStatus(config: MetaFacebookSdkConfig): Promise<FacebookLoginResponse> {
  const fb = await loadMetaFacebookSdk(config)
  return new Promise((resolve) => fb.getLoginStatus(resolve))
}

export async function ensureMetaLogin(config: MetaFacebookSdkConfig): Promise<FacebookLoginResponse> {
  const fb = await loadMetaFacebookSdk(config)
  const status = await getMetaLoginStatus(config)
  if (status.status === 'connected' && status.authResponse?.accessToken) return status

  return new Promise((resolve, reject) => {
    fb.login((response) => {
      if (response.status === 'connected' && response.authResponse?.accessToken) {
        resolve(response)
        return
      }
      reject(new Error('Meta sign-in was not completed.'))
    }, loginOptions(config))
  })
}

function initializeFacebookSdk(fb: FacebookSDK, config: Required<Pick<MetaFacebookSdkConfig, 'appId' | 'apiVersion' | 'locale'>>) {
  fb.init({
    appId: config.appId,
    cookie: true,
    xfbml: true,
    version: config.apiVersion,
  })
  fb.AppEvents?.logPageView?.()
}

function loginOptions(config: MetaFacebookSdkConfig): Record<string, string | boolean> | undefined {
  const loginConfigId = config.loginConfigId?.trim()
  if (!loginConfigId) return undefined
  return { config_id: loginConfigId }
}

function normalizeConfig(config: MetaFacebookSdkConfig): Required<Pick<MetaFacebookSdkConfig, 'appId' | 'apiVersion' | 'locale'>> {
  const appId = config.appId?.trim()
  if (!appId) throw new Error('Meta JavaScript SDK app id is missing.')

  const rawVersion = config.apiVersion?.trim() || 'v23.0'
  const apiVersion = rawVersion.toLowerCase().startsWith('v') ? rawVersion : `v${rawVersion}`
  const locale = /^[a-z]{2}_[A-Z]{2}$/.test(config.locale ?? '') ? config.locale! : 'en_US'
  return { appId, apiVersion, locale }
}
