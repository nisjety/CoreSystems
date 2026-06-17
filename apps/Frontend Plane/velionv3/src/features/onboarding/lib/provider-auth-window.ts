function openProviderAuthWindow(): Window | null {
  const width = Math.min(540, window.screen.width)
  const height = Math.min(720, window.screen.height)
  const left = Math.max(window.screen.width / 2 - width / 2, 0)
  const top = Math.max(window.screen.height / 2 - height / 2, 0)

  return window.open(
    '',
    '_blank',
    [
      `left=${left}`,
      `top=${top}`,
      `width=${width}`,
      `height=${height}`,
      'scrollbars=yes',
      'resizable=yes',
      'status=no',
      'toolbar=no',
      'location=no',
      'copyhistory=no',
      'menubar=no',
      'directories=no',
    ].join(','),
  )
}

export async function runDirectOauthWindow(input: {
  connectUrl: string
  sessionToken?: string
}): Promise<void> {
  const expectedOrigin = new URL(input.connectUrl).origin
  const authWindow = openProviderAuthWindow()
  if (!authWindow) {
    throw new Error('The provider sign-in window was blocked by the browser.')
  }

  await new Promise<void>((resolve, reject) => {
    let settled = false
    const settle = (fn: () => void) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeout)
      window.clearInterval(closePoll)
      window.removeEventListener('message', handleMessage)
      fn()
    }

    const handleMessage = (event: MessageEvent) => {
      if (event.source !== authWindow || event.origin !== expectedOrigin) return
      const payload = event.data
      if (!payload || typeof payload !== 'object') return

      const record = payload as Record<string, unknown>
      if (record.type !== 'velion.integration.connected') return
      if (input.sessionToken && record.sessionToken !== input.sessionToken) return

      if (record.status === 'success') {
        settle(resolve)
        return
      }

      const message =
        typeof record.message === 'string' && record.message.trim()
          ? record.message
          : 'The authorization flow did not complete.'
      settle(() => reject(new Error(message)))
    }

    window.addEventListener('message', handleMessage)

    const timeout = window.setTimeout(() => {
      settle(() => reject(new Error('The authorization flow timed out before the provider returned a result.')))
    }, 120000)

    const closePoll = window.setInterval(() => {
      if (!authWindow.closed) return
      settle(() => reject(new Error('The authorization window was closed before the connection finished.')))
    }, 500)

    try {
      authWindow.location.href = input.connectUrl
    } catch {
      settle(() => reject(new Error('The authorization window was closed before the connection finished.')))
    }
  })
}
