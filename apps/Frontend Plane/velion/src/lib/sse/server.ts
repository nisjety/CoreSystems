export interface SseFrameOptions {
  id?: string
  event?: string
  retry?: number
}

let fallbackCounter = 0

export function createSseEventId(prefix = 'evt'): string {
  fallbackCounter += 1
  return `${prefix}-${Date.now()}-${fallbackCounter}`
}

export function encodeSseFrame(payload: unknown, options: SseFrameOptions = {}): string {
  const lines: string[] = []

  if (options.id) {
    lines.push(`id: ${options.id}`)
  }

  if (options.event) {
    lines.push(`event: ${options.event}`)
  }

  if (typeof options.retry === 'number') {
    lines.push(`retry: ${options.retry}`)
  }

  const data = typeof payload === 'string' ? payload : JSON.stringify(payload)
  for (const line of data.split(/\r?\n/)) {
    lines.push(`data: ${line}`)
  }

  return `${lines.join('\n')}\n\n`
}

export function encodeSseChunk(
  payload: unknown,
  encoder: TextEncoder,
  options: SseFrameOptions = {},
): Uint8Array {
  return encoder.encode(encodeSseFrame(payload, options))
}
