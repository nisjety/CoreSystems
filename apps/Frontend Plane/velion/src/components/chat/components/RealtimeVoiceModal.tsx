'use client';

/**
 * U2-15 follow-up (velion ui-ux-velion-gap.md §10): Realtime voice modal.
 *
 * Wires the chat composer's "voice mode" button to the model-gateway's
 * `/v1/ai/realtime` WebSocket. The browser opens the WebSocket directly
 * (Next.js doesn't proxy WS in the App Router) — the URL and bearer are
 * fetched from `/api/ai/realtime` first.
 *
 * Wire shape (one full turn):
 *   client → server:
 *     { type: 'audio', mime: 'audio/webm', data: '<base64>' }
 *   server → client (in order):
 *     { type: 'transcript', text: '...' }            // STT result
 *     { type: 'assistant_text', content: '...', model }
 *     { type: 'assistant_audio', mime: 'audio/mpeg', data: '<base64>' }
 *     { type: 'turn_complete' }
 *
 * Recording uses MediaRecorder with `audio/webm` and streams the blob as
 * base64 once the user releases the record button. The assistant audio
 * is rendered via an HTMLAudioElement fed with a `data:` URL.
 */

import { useCallback, useEffect, useRef, useState, type ReactElement } from 'react'
import { createPortal } from 'react-dom'
import { Mic, Square, X, Volume2, VolumeX, MessageSquare } from 'lucide-react'
import { m, AnimatePresence } from 'framer-motion'
import { cn } from '@/lib/utils'

interface RealtimeVoiceModalProps {
  open: boolean
  onClose: () => void
  /** Optional model override forwarded on every audio turn. */
  model?: string
  /** Optional system prompt set on every turn. */
  systemPrompt?: string
  /** Spoken-response language (BCP-47). Defaults to en-US. */
  language?: string
  /** Azure TTS voice id (default nb-NO-FinnNeural). */
  voice?: string
}

interface TurnEntry {
  id: string
  role: 'user' | 'assistant'
  text: string
  /** Object URL for the assistant audio (cleaned on close). */
  audioUrl?: string
}

interface RealtimeConfig {
  url: string
  protocols: string[]
  expires_in: number
}

type ServerMessage =
  | { type: 'ready'; session_id: string }
  | { type: 'transcript'; text: string }
  | { type: 'assistant_text'; content: string; model: string }
  | { type: 'assistant_audio'; mime: string; data: string }
  | { type: 'turn_complete' }
  | { type: 'error'; detail: string }

function base64ToBlob(b64: string, mime: string): Blob {
  const binary = atob(b64)
  const len = binary.length
  const bytes = new Uint8Array(len)
  for (let i = 0; i < len; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return new Blob([bytes], { type: mime })
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onloadend = () => {
      const result = typeof reader.result === 'string' ? reader.result : ''
      // Strip "data:<mime>;base64," prefix.
      const idx = result.indexOf(',')
      resolve(idx === -1 ? result : result.slice(idx + 1))
    }
    reader.onerror = () => reject(reader.error ?? new Error('FileReader failed'))
    reader.readAsDataURL(blob)
  })
}

export function RealtimeVoiceModal({
  open,
  onClose,
  model,
  systemPrompt,
  language = 'en-US',
  voice = 'nb-NO-FinnNeural',
}: RealtimeVoiceModalProps): ReactElement | null {
  const [mounted, setMounted] = useState(false)
  const [status, setStatus] = useState<'idle' | 'connecting' | 'ready' | 'recording' | 'thinking' | 'speaking' | 'error' | 'closed'>('idle')
  const [error, setError] = useState<string | null>(null)
  const [turns, setTurns] = useState<TurnEntry[]>([])
  const [autoplay, setAutoplay] = useState(true)

  const wsRef = useRef<WebSocket | null>(null)
  const mediaRecorderRef = useRef<MediaRecorder | null>(null)
  const audioStreamRef = useRef<MediaStream | null>(null)
  const audioChunksRef = useRef<Blob[]>([])
  const audioElementRef = useRef<HTMLAudioElement | null>(null)

  // Track which assistant turn we're filling in (transcript → text → audio
  // arrive across three frames, but they belong to one turn).
  const currentTurnIdRef = useRef<string | null>(null)

  useEffect(() => {
    setMounted(true)
    return () => setMounted(false)
  }, [])

  // -------- Cleanup helpers --------

  const closeSocket = useCallback(() => {
    if (wsRef.current && wsRef.current.readyState <= WebSocket.OPEN) {
      try {
        wsRef.current.send(JSON.stringify({ type: 'end' }))
      } catch {
        /* socket already closed */
      }
      try {
        wsRef.current.close()
      } catch {
        /* swallow */
      }
    }
    wsRef.current = null
  }, [])

  const stopMicrophone = useCallback(() => {
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      try {
        mediaRecorderRef.current.stop()
      } catch {
        /* swallow */
      }
    }
    if (audioStreamRef.current) {
      audioStreamRef.current.getTracks().forEach((t) => t.stop())
      audioStreamRef.current = null
    }
    mediaRecorderRef.current = null
  }, [])

  const teardown = useCallback(() => {
    stopMicrophone()
    closeSocket()
    setTurns((prev) => {
      prev.forEach((t) => {
        if (t.audioUrl) URL.revokeObjectURL(t.audioUrl)
      })
      return []
    })
    setStatus('closed')
  }, [closeSocket, stopMicrophone])

  // -------- WebSocket lifecycle --------

  const openSession = useCallback(async () => {
    setError(null)
    setStatus('connecting')
    try {
      const res = await fetch('/api/ai/realtime', { cache: 'no-store' })
      if (!res.ok) {
        throw new Error(`config: HTTP ${res.status}`)
      }
      const config = (await res.json()) as RealtimeConfig

      const ws = new WebSocket(config.url, config.protocols)
      wsRef.current = ws

      ws.onopen = () => {
        // status moves to 'ready' once we receive the server's
        // `{type:'ready'}` frame — keeping the UI honest.
      }
      ws.onerror = () => {
        setError('WebSocket error — check model-gateway is reachable.')
        setStatus('error')
      }
      ws.onclose = (ev) => {
        if (status !== 'error') {
          setStatus(ev.wasClean ? 'closed' : 'error')
        }
      }
      ws.onmessage = (evt) => {
        let parsed: ServerMessage | null = null
        try {
          parsed = JSON.parse(typeof evt.data === 'string' ? evt.data : '') as ServerMessage
        } catch {
          return
        }
        if (!parsed) return

        switch (parsed.type) {
          case 'ready':
            setStatus('ready')
            break
          case 'transcript': {
            const trimmed = parsed.text.trim()
            if (trimmed.length > 0) {
              setTurns((prev) => [
                ...prev,
                { id: `u-${Date.now()}`, role: 'user', text: trimmed },
              ])
            }
            const id = `a-${Date.now()}`
            currentTurnIdRef.current = id
            setTurns((prev) => [...prev, { id, role: 'assistant', text: '' }])
            setStatus('thinking')
            break
          }
          case 'assistant_text': {
            const id = currentTurnIdRef.current
            if (!id) break
            setTurns((prev) =>
              prev.map((t) => (t.id === id ? { ...t, text: parsed!.type === 'assistant_text' ? parsed.content : t.text } : t)),
            )
            break
          }
          case 'assistant_audio': {
            const id = currentTurnIdRef.current
            if (!id || parsed.type !== 'assistant_audio') break
            const blob = base64ToBlob(parsed.data, parsed.mime || 'audio/mpeg')
            const url = URL.createObjectURL(blob)
            setTurns((prev) =>
              prev.map((t) => (t.id === id ? { ...t, audioUrl: url } : t)),
            )
            setStatus('speaking')
            if (autoplay && audioElementRef.current) {
              audioElementRef.current.src = url
              void audioElementRef.current.play().catch(() => {
                /* autoplay may be blocked — user can hit play manually */
              })
            }
            break
          }
          case 'turn_complete':
            setStatus('ready')
            currentTurnIdRef.current = null
            break
          case 'error':
            setError(parsed.detail)
            setStatus('error')
            break
          default:
            break
        }
      }
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : 'unknown'
      setError(`Failed to open realtime session: ${detail}`)
      setStatus('error')
    }
  }, [autoplay, status])

  // Open on mount, teardown on unmount/close.
  useEffect(() => {
    if (!open) return
    void openSession()
    return () => {
      teardown()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // -------- Recording handlers --------

  const startRecording = useCallback(async () => {
    if (status !== 'ready' || !wsRef.current) return
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      audioStreamRef.current = stream
      const recorder = new MediaRecorder(stream, { mimeType: 'audio/webm' })
      mediaRecorderRef.current = recorder
      audioChunksRef.current = []

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) audioChunksRef.current.push(e.data)
      }
      recorder.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop())
        audioStreamRef.current = null
        const blob = new Blob(audioChunksRef.current, { type: 'audio/webm' })
        audioChunksRef.current = []
        if (blob.size === 0) {
          setStatus('ready')
          return
        }
        try {
          const data = await blobToBase64(blob)
          wsRef.current?.send(
            JSON.stringify({
              type: 'audio',
              mime: 'audio/webm',
              data,
              model,
              system_prompt: systemPrompt,
              language,
              voice,
            }),
          )
          setStatus('thinking')
        } catch (e: unknown) {
          const detail = e instanceof Error ? e.message : 'unknown'
          setError(`Failed to send audio: ${detail}`)
          setStatus('error')
        }
      }
      recorder.start()
      setStatus('recording')
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : 'permission denied'
      setError(`Microphone unavailable: ${detail}`)
      setStatus('error')
    }
  }, [language, model, status, systemPrompt, voice])

  const stopRecording = useCallback(() => {
    if (!mediaRecorderRef.current) return
    if (mediaRecorderRef.current.state !== 'inactive') {
      mediaRecorderRef.current.stop()
    }
  }, [])

  // -------- UI --------

  if (!mounted) return null

  const statusLabel = (() => {
    switch (status) {
      case 'idle':
        return 'Idle'
      case 'connecting':
        return 'Connecting…'
      case 'ready':
        return 'Ready — hold the mic to talk'
      case 'recording':
        return 'Recording…'
      case 'thinking':
        return 'Thinking…'
      case 'speaking':
        return 'Speaking…'
      case 'error':
        return error ?? 'Error'
      case 'closed':
        return 'Closed'
      default:
        return ''
    }
  })()

  const modal = (
    <AnimatePresence>
      {open && (
        <m.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-[120] flex items-center justify-center bg-black/40 backdrop-blur-sm"
          onClick={() => {
            teardown()
            onClose()
          }}
        >
          <m.div
            initial={{ scale: 0.96, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.96, opacity: 0 }}
            transition={{ duration: 0.16 }}
            onClick={(e) => e.stopPropagation()}
            className="w-[min(560px,92vw)] max-h-[86vh] flex flex-col bg-white rounded-2xl shadow-2xl border border-black/5 overflow-hidden"
          >
            <header className="flex items-center justify-between px-5 py-3.5 border-b border-black/5">
              <div className="flex items-center gap-2">
                <div
                  className={cn(
                    'w-2 h-2 rounded-full transition-colors',
                    status === 'ready' && 'bg-green-500',
                    status === 'connecting' && 'bg-amber-400 animate-pulse',
                    status === 'recording' && 'bg-rose-500 animate-pulse',
                    status === 'thinking' && 'bg-violet-500 animate-pulse',
                    status === 'speaking' && 'bg-sky-500 animate-pulse',
                    (status === 'idle' || status === 'closed') && 'bg-zinc-300',
                    status === 'error' && 'bg-red-500',
                  )}
                />
                <span className="text-[14px] font-semibold tracking-tight">
                  Voice mode
                </span>
                <span className="text-[12px] text-zinc-500 ml-1">
                  {statusLabel}
                </span>
              </div>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => setAutoplay((v) => !v)}
                  className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-900 hover:bg-black/5 transition-colors"
                  aria-label={autoplay ? 'Mute autoplay' : 'Enable autoplay'}
                  title={autoplay ? 'Autoplay on' : 'Autoplay off'}
                >
                  {autoplay ? <Volume2 size={16} /> : <VolumeX size={16} />}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    teardown()
                    onClose()
                  }}
                  className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-900 hover:bg-black/5 transition-colors"
                  aria-label="Close"
                >
                  <X size={16} />
                </button>
              </div>
            </header>

            <main className="flex-1 min-h-[200px] max-h-[60vh] overflow-y-auto px-5 py-4 bg-zinc-50/50">
              {turns.length === 0 ? (
                <div className="h-full flex flex-col items-center justify-center text-zinc-400 py-12">
                  <MessageSquare size={28} strokeWidth={1.5} className="mb-2" />
                  <p className="text-[13px]">
                    Hold the mic, speak, then release.
                  </p>
                  <p className="text-[12px] mt-1 opacity-70">
                    Replies arrive as text + spoken audio.
                  </p>
                </div>
              ) : (
                <ul className="flex flex-col gap-2.5">
                  {turns.map((t) => (
                    <li
                      key={t.id}
                      className={cn(
                        'rounded-2xl px-3.5 py-2 max-w-[88%] text-[14px] leading-snug',
                        t.role === 'user'
                          ? 'self-end bg-zinc-900 text-white'
                          : 'self-start bg-white border border-black/5 text-zinc-800',
                      )}
                    >
                      {t.text || (t.role === 'assistant' ? '…' : '')}
                      {t.audioUrl && (
                        <audio
                          controls
                          src={t.audioUrl}
                          className="mt-2 w-full h-8"
                        />
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </main>

            <footer className="px-5 py-4 border-t border-black/5 bg-white flex items-center justify-between gap-3">
              <button
                type="button"
                onMouseDown={startRecording}
                onMouseUp={stopRecording}
                onMouseLeave={() => {
                  if (status === 'recording') stopRecording()
                }}
                onTouchStart={(e) => {
                  e.preventDefault()
                  void startRecording()
                }}
                onTouchEnd={(e) => {
                  e.preventDefault()
                  stopRecording()
                }}
                disabled={status !== 'ready' && status !== 'recording'}
                className={cn(
                  'flex-1 flex items-center justify-center gap-2 h-12 rounded-xl font-medium transition-all select-none',
                  status === 'recording'
                    ? 'bg-rose-500 text-white shadow-md'
                    : status === 'ready'
                      ? 'bg-zinc-900 text-white hover:bg-zinc-800 shadow-md'
                      : 'bg-zinc-100 text-zinc-400 cursor-not-allowed',
                )}
                aria-pressed={status === 'recording'}
                aria-label={status === 'recording' ? 'Release to send' : 'Hold to talk'}
              >
                {status === 'recording' ? (
                  <>
                    <Square size={16} fill="currentColor" />
                    Release to send
                  </>
                ) : (
                  <>
                    <Mic size={16} />
                    {status === 'ready' ? 'Hold to talk' : 'Connecting…'}
                  </>
                )}
              </button>
            </footer>

            {/* Hidden audio element used for autoplay. */}
            <audio ref={audioElementRef} className="hidden" />
          </m.div>
        </m.div>
      )}
    </AnimatePresence>
  )

  return createPortal(modal, document.body)
}
