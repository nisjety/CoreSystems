// The COOP-safe implementation lives in shared/integrations so the settings
// surface reuses the exact same popup + status-polling flow.
export { reserveDirectOauthWindow, runDirectOauthWindow } from '@/shared/integrations/provider-auth-window'
