export interface SseEvent {
  id?: string
  event?: string
  data: string
  retry?: number
}

export interface ParsedSseChunk {
  events: SseEvent[]
  buffer: string
}

export function parseSseChunk(previousBuffer: string, chunk: string): ParsedSseChunk {
  const normalized = `${previousBuffer}${chunk}`.replace(/\r\n/g, '\n')
  const frames = normalized.split('\n\n')
  const buffer = frames.pop() ?? ''
  const events = frames.map(parseSseFrame).filter((event): event is SseEvent => event !== null)

  return { events, buffer }
}

export function parseSseFrame(frame: string): SseEvent | null {
  const event: SseEvent = { data: '' }
  const dataLines: string[] = []

  for (const rawLine of frame.split('\n')) {
    if (!rawLine || rawLine.startsWith(':')) {
      continue
    }

    const separatorIndex = rawLine.indexOf(':')
    const field = separatorIndex === -1 ? rawLine : rawLine.slice(0, separatorIndex)
    const rawValue = separatorIndex === -1 ? '' : rawLine.slice(separatorIndex + 1)
    const value = rawValue.startsWith(' ') ? rawValue.slice(1) : rawValue

    if (field === 'id') {
      event.id = value
    } else if (field === 'event') {
      event.event = value
    } else if (field === 'data') {
      dataLines.push(value)
    } else if (field === 'retry') {
      const retry = Number(value)
      if (Number.isFinite(retry)) {
        event.retry = retry
      }
    }
  }

  if (dataLines.length === 0) {
    return null
  }

  return {
    ...event,
    data: dataLines.join('\n'),
  }
}

export function parseJsonSseData<T>(event: SseEvent): T | null {
  if (!event.data.trim() || event.data.trim() === '[DONE]') {
    return null
  }

  try {
    return JSON.parse(event.data) as T
  } catch {
    return null
  }
}
