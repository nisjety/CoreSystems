import { A, useNavigate } from '@solidjs/router'
import {
  AlignCenter,
  AlignJustify,
  AlignLeft,
  ArrowLeft,
  ArrowUp,
  AudioWaveform,
  Blocks,
  Briefcase,
  Camera,
  Calendar,
  Check,
  ChevronDown,
  ChevronRight,
  CirclePlus,
  Clock3,
  FileText,
  FolderPlus,
  Globe2,
  ImagePlus,
  Lightbulb,
  LayoutGrid,
  Loader2,
  Maximize2,
  MessageSquare,
  Minimize2,
  Mic,
  Paperclip,
  SlidersHorizontal,
  Sparkles,
  Square,
  Telescope,
  Upload,
  User,
  WandSparkles,
  X,
  Zap,
  type LucideProps,
} from 'lucide-solid'
import { batch, createEffect, createMemo, createResource, createSignal, For, Match, onCleanup, Show, Switch, untrack, type Component, type JSX } from 'solid-js'
import { Dynamic, Portal } from 'solid-js/web'
import { transcribeAudioBlob } from '@/shared/api/audio-client'
import {
  actionKey,
  BUILTIN_ACTIONS,
  loadSpecializedActions,
  type SpecializedAction,
  type SpecializedActionKind,
} from '@/shared/api/chat-actions-client'
import { loadComposerSettingsItems, type ComposerSettingsItem } from '@/shared/api/composer-settings-client'
import { searchNavbar } from '@/shared/api/navbar-client'
import { selectChatThread } from '@/features/chat/lib/chat-thread-history'
import { writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'
import {
  groupChatModels,
  isExpensiveModel,
  listChatThreads,
  listModels,
  velionModeById,
  VELION_BALANCE_MODE_ID,
  VELION_MODES,
  type ChatThreadSession,
  type ModelGroup,
  type ModelInfo,
} from '@/shared/api/chat-client'
import { cn } from '@/shared/lib/cn'

type ResponseMode = 'auto' | 'quick' | 'deep'

type ResponseModeOption = {
  announcement: string
  icon: Component<LucideProps>
  id: ResponseMode
  label: string
}

type ComposerTone = 'concise' | 'balanced' | 'detailed'

type ComposerFile = {
  id: string
  name: string
  size: number
  type: string
  url: string
}

type ComposerTurn = {
  id: string
  body: string
  browseWeb: boolean
  createdAt: string
  createdAtIso: string
  deepSearch: boolean
  files: string[]
  model: string
  responseMode: ResponseMode
}

type HistoryPanelItem = {
  fallbackTime: string
  id: string
  meta: string
  threadId?: string
  title: string
  updatedAt: string
}

type ComposerSettings = {
  tone: ComposerTone
  voiceLang: string
}

type SettingsView = 'main' | 'skills' | 'projects' | 'connectors'
type EntityKind = 'date' | 'file' | 'person'

type EntityToken = {
  kind: EntityKind
  start: number
  text: string
}

type AutocompleteItem = {
  action?: 'file' | 'image'
  icon: Component<LucideProps>
  id: string
  label: string
  meta?: EntityKind | 'slash'
  specialized?: SpecializedAction
}

type AutocompleteState = {
  category: string
  items: AutocompleteItem[]
  triggerLen: number
  triggerStart: number
}

type TriggerContext =
  | { type: 'date'; dayIndex: number; rawLen: number; start: number }
  | { type: 'person'; query: string; rawLen: number; start: number }
  | { type: 'slash'; query: string; rawLen: number; start: number }

type DashboardComposerAttachment = {
  id: string
  name: string
  size: number
  type: string
  url: string
}

/** A skill/capability/connector activated via the `/` menu. */
export type ComposerActiveAction = {
  id: string
  name: string
  kind: 'skill' | 'capability' | 'connector'
}

export type DashboardComposerSubmitPayload = {
  actions: ComposerActiveAction[]
  attachments: DashboardComposerAttachment[]
  model?: string
  text: string
  tools: Array<'image' | 'reason' | 'research' | 'search'>
}

type PanelPosition = {
  bottom?: number
  left?: number
  maxHeight: number
  right?: number
  top?: number
}

const responseModes: ResponseModeOption[] = [
  { id: 'auto', label: 'Auto', announcement: 'Auto mode', icon: WandSparkles },
  { id: 'quick', label: 'Quick response', announcement: 'Quick response activated', icon: Zap },
  { id: 'deep', label: 'Deep research', announcement: 'Deep research mode', icon: Lightbulb },
]
const voiceLanguages = [
  { value: 'en-US', label: 'English (US)' },
  { value: 'en-GB', label: 'English (UK)' },
  { value: 'nb-NO', label: 'Norwegian (Bokmål)' },
  { value: 'nn-NO', label: 'Norwegian (Nynorsk)' },
  { value: 'sv-SE', label: 'Swedish' },
  { value: 'da-DK', label: 'Danish' },
  { value: 'de-DE', label: 'German' },
  { value: 'fr-FR', label: 'French' },
  { value: 'es-ES', label: 'Spanish' },
  { value: 'pt-BR', label: 'Portuguese (BR)' },
] as const
const toneOptions: Array<{ value: ComposerTone; label: string; icon: Component<LucideProps> }> = [
  { value: 'concise', label: 'Concise', icon: AlignLeft },
  { value: 'balanced', label: 'Balanced', icon: AlignCenter },
  { value: 'detailed', label: 'Detailed', icon: AlignJustify },
]
const dayEntries = [
  { name: 'Monday', dayIndex: 1 },
  { name: 'Tuesday', dayIndex: 2 },
  { name: 'Wednesday', dayIndex: 3 },
  { name: 'Thursday', dayIndex: 4 },
  { name: 'Friday', dayIndex: 5 },
  { name: 'Saturday', dayIndex: 6 },
  { name: 'Sunday', dayIndex: 0 },
] as const
const slashCommands: AutocompleteItem[] = [
  { id: 'cmd-file', icon: Upload, label: 'File upload', meta: 'slash', action: 'file' },
  { id: 'cmd-image', icon: ImagePlus, label: 'Generate image', meta: 'slash', action: 'image' },
]

const TEXTAREA_AUTO_MAX_PX = 240

type ComposerSubmitState = {
  hasContent: boolean
  submitting?: boolean
  voiceMode: boolean
  voiceRecording: boolean
}

function dashboardComposerRootClass(input: { dragActive: boolean }) {
  return cn(
    'dashboard-composer-root',
    input.dragActive ? 'dashboard-composer-root--drag-active' : '',
  )
}

function composerChevronClass(open: boolean) {
  return cn(
    'size-4 dashboard-composer-model-button__chevron',
    open ? 'dashboard-composer-model-button__chevron--open' : '',
  )
}

function composerWebButtonClass(active: boolean) {
  return cn('dashboard-composer-web-button', active ? 'dashboard-composer-web-button--active' : '')
}

function composerImageButtonClass(active: boolean) {
  return cn('dashboard-composer-image-button', active ? 'dashboard-composer-image-button--active' : '')
}

function composerSubmitButtonClass(enabled: boolean) {
  return cn('dashboard-composer-submit-button', enabled ? 'dashboard-composer-submit-button--active' : '')
}

function textareaOverlayStyle(hasEntities: boolean): JSX.CSSProperties {
  return {
    color: hasEntities ? 'transparent' : undefined,
    'caret-color': 'currentColor',
  }
}

function modeAnnouncementDisplay(announcement: string | null, message: string) {
  if (!announcement || message) return ''
  return announcement
}

function textareaPlaceholder(announcement: string | null) {
  return announcement ? '' : 'Ask anything, use / to activate specialized actions.'
}

function textareaCursorPosition(element: HTMLTextAreaElement) {
  return element.selectionStart ?? element.value.length
}

function shouldShowTextareaExpandButton(hasOverflow: boolean, expanded: boolean) {
  return hasOverflow || expanded
}

function textareaExpandLabel(expanded: boolean) {
  return expanded ? 'Collapse input' : 'Expand to see full text'
}

function textareaExpandTitle(expanded: boolean) {
  return expanded ? 'Collapse' : 'Expand'
}

function isVoiceInputLocked(voiceMode: boolean, voiceRecording: boolean) {
  return voiceMode || voiceRecording
}

function isComposerSubmitEnabled(input: ComposerSubmitState) {
  return input.hasContent && !isVoiceInputLocked(input.voiceMode, input.voiceRecording) && !input.submitting
}

function shouldShowStopButton(submitting: boolean | undefined, onStop: (() => void) | undefined) {
  return Boolean(submitting && onStop)
}

function visibleTurnReceipt(showTurnReceipt: boolean | undefined, turn: ComposerTurn | undefined) {
  return (showTurnReceipt ?? true) ? turn : undefined
}

function dragEventHasFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.types ?? []).includes('Files')
}

function droppedFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
}

function pastedImageFiles(event: ClipboardEvent) {
  return Array.from(event.clipboardData?.files ?? []).filter((file) => file.type.startsWith('image/'))
}

export function DashboardComposer(props: {
  browseWeb?: boolean
  imageMode?: boolean
  message: string
  onBrowseWebChange?: (value: boolean) => void
  onImageModeChange?: (value: boolean) => void
  onMessageChange: (value: string) => void
  onPlanModeChange?: (value: boolean) => void
  planMode?: boolean
  onStop?: () => void
  onLaunchStart?: () => void
  onSubmit?: (payload: DashboardComposerSubmitPayload) => Promise<void> | void
  showTurnReceipt?: boolean
  submitting?: boolean
}) {
  const navigate = useNavigate()
  let composerRootRef: HTMLDivElement | undefined
  let fileInputRef: HTMLInputElement | undefined
  let historyTriggerRef: HTMLSpanElement | undefined
  let settingsTriggerRef: HTMLSpanElement | undefined
  let textareaRef: HTMLTextAreaElement | undefined
  let autocompleteTimer: number | undefined
  let autocompleteController: AbortController | undefined
  let modeAnnouncementTimer: number | undefined
  let mediaRecorder: MediaRecorder | undefined
  let audioChunks: Blob[] = []
  let pendingFilePosition: number | null = null
  const [internalBrowseWeb, setInternalBrowseWeb] = createSignal(false)
  const browseWeb = () => props.browseWeb ?? internalBrowseWeb()
  const setBrowseWeb = (next: boolean | ((current: boolean) => boolean)) => {
    const value = typeof next === 'function' ? next(browseWeb()) : next
    if (props.browseWeb === undefined) setInternalBrowseWeb(value)
    props.onBrowseWebChange?.(value)
  }
  const [autocomplete, setAutocomplete] = createSignal<AutocompleteState | null>(null)
  const [autocompleteIndex, setAutocompleteIndex] = createSignal(0)
  const [deepSearch, setDeepSearch] = createSignal(false)
  const [dragActive, setDragActive] = createSignal(false)
  const [entities, setEntities] = createSignal<EntityToken[]>([])
  const [files, setFiles] = createSignal<ComposerFile[]>([])
  const [historyPanelPosition, setHistoryPanelPosition] = createSignal<PanelPosition>({ bottom: 0, right: 0, maxHeight: 400 })
  const [historyThreads, setHistoryThreads] = createSignal<ChatThreadSession[]>([])
  const [historyLoading, setHistoryLoading] = createSignal(false)
  const [historyError, setHistoryError] = createSignal<string | null>(null)
  const [historyOpen, setHistoryOpen] = createSignal(false)
  const [modeAnnouncement, setModeAnnouncement] = createSignal<string | null>(null)
  const [modelOpen, setModelOpen] = createSignal(false)
  const [responseMode, setResponseMode] = createSignal<ResponseMode>('auto')
  // The picker pins three first-class Velion intent modes (Budget/Balance/Genius)
  // at the very top; the backend resolves these pseudo-model ids server-side. The
  // real catalog models come live from the gateway `/api/v1/models` (Model Plane)
  // and render below. Default = Velion Balance. We never auto-select an expensive
  // catalog model; users pick opus/sonnet/gpt-5.x deliberately (cost badge nudges).
  const [selectedModel, setSelectedModel] = createSignal<string>(VELION_BALANCE_MODE_ID)
  const [models] = createResource(async () => {
    try {
      return await listModels()
    } catch {
      // Gateway unavailable / old shape → empty catalog. The pinned Velion intent
      // modes still render (they don't depend on /v1/models), so the picker works.
      return [] as ModelInfo[]
    }
  })
  // Chat-capable models grouped by family/provider. Non-chat modalities
  // (image/video/embeddings/transcribe) are filtered out by `groupChatModels`.
  const chatModelGroups = createMemo<ModelGroup[]>(() => groupChatModels(models() ?? []))
  const flatChatModels = createMemo<ModelInfo[]>(() =>
    chatModelGroups().flatMap((group) => group.models),
  )
  const selectedModelLabel = () => {
    const id = selectedModel()
    // Empty id → backend treats it as Velion Balance (resilient fallback).
    if (!id) return 'Velion Balance'
    return velionModeById(id)?.label ?? flatChatModels().find((model) => model.id === id)?.name ?? id
  }
  const selectModel = (id: string) => {
    setSelectedModel(id)
    setModelOpen(false)
  }
  const [settings, setSettings] = createSignal<ComposerSettings>({ tone: 'balanced', voiceLang: 'en-US' })
  const [settingsPanelPosition, setSettingsPanelPosition] = createSignal<PanelPosition>({ top: 0, left: 0, maxHeight: 460 })
  const [settingsOpen, setSettingsOpen] = createSignal(false)
  const [suggestionsOpen, setSuggestionsOpen] = createSignal(false)
  const [turns, setTurns] = createSignal<ComposerTurn[]>([])
  const [voiceMode, setVoiceMode] = createSignal(false)
  const [voiceRecording, setVoiceRecording] = createSignal(false)
  const hasContent = createMemo(() => props.message.trim().length > 0 || files().length > 0)
  const imageMode = createMemo(() => props.imageMode ?? false)
  const planMode = createMemo(() => props.planMode ?? false)
  const hasEntityOverlay = createMemo(() => entities().length > 0)
  const [textareaExpanded, setTextareaExpanded] = createSignal(false)
  const [hasOverflow, setHasOverflow] = createSignal(false)
  const currentModeAnnouncement = createMemo(() => modeAnnouncementDisplay(modeAnnouncement(), props.message))
  const textareaLocked = createMemo(() => isVoiceInputLocked(voiceMode(), voiceRecording()))
  const showTextareaExpand = createMemo(() => shouldShowTextareaExpandButton(hasOverflow(), textareaExpanded()))
  const submitEnabled = createMemo(() =>
    isComposerSubmitEnabled({
      hasContent: hasContent(),
      submitting: props.submitting,
      voiceMode: voiceMode(),
      voiceRecording: voiceRecording(),
    }),
  )
  const stopButtonVisible = createMemo(() => shouldShowStopButton(props.submitting, props.onStop))
  const activeTurnReceipt = createMemo(() => visibleTurnReceipt(props.showTurnReceipt, turns()[0]))
  const [activeActions, setActiveActions] = createSignal<ComposerActiveAction[]>([])
  const [specializedActions, setSpecializedActions] = createSignal<SpecializedAction[]>(BUILTIN_ACTIONS)
  let actionsLoadStarted = false

  const openFileDialog = () => fileInputRef?.click()
  const focusTextareaAt = (caret?: number) => {
    const element = textareaRef
    if (!element) return
    element.focus()
    if (typeof caret === 'number') {
      element.setSelectionRange(caret, caret)
    }
  }
  const focusTextareaAtNextFrame = (caret?: number) => {
    window.requestAnimationFrame(() => focusTextareaAt(caret))
  }

  // Lazily fetch skills/capabilities/connectors the first time `/` is used, so
  // the dashboard doesn't pay for the call until the menu is actually opened.
  const ensureActionsLoaded = () => {
    if (actionsLoadStarted) return
    actionsLoadStarted = true
    void loadSpecializedActions()
      .then((items) => {
        setSpecializedActions(items.length > 0 ? items : BUILTIN_ACTIONS)
        untrack(() => {
          const state = autocomplete()
          if (state?.category === 'Actions') {
            const query = props.message.slice(state.triggerStart + 1, state.triggerStart + state.triggerLen)
            openActionMenu(query, state.triggerStart, state.triggerLen)
          }
        })
      })
      .catch(() => undefined)
  }

  const buildSlashItems = (query: string): AutocompleteItem[] => {
    const normalized = query.trim().toLowerCase()
    const active = activeActions()
    return specializedActions()
      .filter((action) => action.kind === 'builtin' || !active.some((a) => a.id === action.id && a.kind === action.kind))
      .filter((action) =>
        normalized.length === 0 ||
        action.name.toLowerCase().includes(normalized) ||
        action.id.toLowerCase().includes(normalized),
      )
      .slice(0, 8)
      .map((action) => ({
        id: actionKey(action),
        icon: iconForAction(action),
        label: action.name,
        meta: 'slash' as const,
        specialized: action,
      }))
  }

  const openActionMenu = (query: string, triggerStart: number, triggerLen: number) => {
    const items = buildSlashItems(query)
    setAutocompleteIndex(0)
    setAutocomplete(
      items.length > 0
        ? { category: 'Actions', items, triggerStart, triggerLen }
        : null,
    )
  }

  const removeActiveAction = (target: ComposerActiveAction) => {
    setActiveActions((current) => current.filter((a) => !(a.id === target.id && a.kind === target.kind)))
  }

  const activateSpecialized = (action: SpecializedAction) => {
    if (action.kind === 'builtin') {
      if (action.builtin === 'file') openFileDialog()
      else if (action.builtin === 'image') props.onImageModeChange?.(true)
      else if (action.builtin === 'web_search') setBrowseWeb(true)
      return
    }
    const kind = action.kind
    setActiveActions((current) =>
      current.some((a) => a.id === action.id && a.kind === kind)
        ? current
        : [...current, { id: action.id, name: action.name, kind }],
    )
  }

  createEffect(() => {
    const element = textareaRef
    const message = props.message
    if (!element) return
    element.style.height = 'auto'
    const naturalH = element.scrollHeight
    const overflows = naturalH > TEXTAREA_AUTO_MAX_PX
    setHasOverflow(overflows)
    if (textareaExpanded()) {
      const maxH = Math.floor(window.innerHeight * 0.5)
      element.style.height = `${Math.min(naturalH, maxH)}px`
      element.style.overflowY = naturalH > maxH ? 'auto' : 'hidden'
    } else {
      element.style.height = `${overflows ? TEXTAREA_AUTO_MAX_PX : naturalH}px`
      element.style.overflowY = overflows ? 'auto' : 'hidden'
    }
    void message
  })

  createEffect(() => {
    if (!modelOpen() && !historyOpen() && !settingsOpen() && !suggestionsOpen() && !autocomplete()) return

    const closePanels = () => {
      setModelOpen(false)
      setHistoryOpen(false)
      setSettingsOpen(false)
      setSuggestionsOpen(false)
      setAutocomplete(null)
    }
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target
      if (!(target instanceof Node)) return
      if (composerRootRef?.contains(target)) return
      if (target instanceof Element && target.closest('[data-composer-floating-panel="true"]')) return
      if (target instanceof Element && target.closest('[data-dashboard-modal="true"]')) return
      closePanels()
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePanels()
    }

    document.addEventListener('pointerdown', handlePointerDown, true)
    document.addEventListener('keydown', handleKeyDown)
    onCleanup(() => {
      document.removeEventListener('pointerdown', handlePointerDown, true)
      document.removeEventListener('keydown', handleKeyDown)
    })
  })

  const addFiles = (nextFiles: FileList | File[]) => {
    const incoming = Array.from(nextFiles)
    if (pendingFilePosition !== null && incoming[0]) {
      const file = incoming[0]
      const position = pendingFilePosition
      const nextMessage = `${props.message.slice(0, position)}${file.name}${props.message.slice(position)}`
      pendingFilePosition = null
      props.onMessageChange(nextMessage)
      setEntities((current) => [...current, { kind: 'file', start: position, text: file.name }])
      setAutocomplete(null)
      focusTextareaAtNextFrame(position + file.name.length)
      return
    }

    const created = Array.from(nextFiles).map((file, index) => ({
      id: `${file.name}-${file.size}-${file.lastModified}-${Date.now()}-${index}`,
      name: file.name,
      size: file.size,
      type: file.type || 'application/octet-stream',
      url: createFilePreviewUrl(file),
    }))

    if (created.length > 0) {
      setFiles((current) => [...current, ...created])
    }
  }

  const removeFile = (id: string) => {
    setFiles((current) => {
      const removed = current.find((file) => file.id === id)
      if (removed) revokeFilePreviewUrl(removed)
      return current.filter((file) => file.id !== id)
    })
  }

  const enhanceAttachments = () => {
    if (files().length === 0) return

    const names = files().map((file) => file.name).join(', ')
    const body = props.message.trim()
    props.onMessageChange(body ? `Analyze the attached file(s) (${names}) and ${body}` : `Describe and analyze the attached file(s): ${names}`)
    focusTextareaAt()
  }

  const toggleRecording = async () => {
    if (voiceRecording()) {
      mediaRecorder?.stop()
      return
    }

    if (typeof MediaRecorder === 'undefined' || !navigator.mediaDevices?.getUserMedia) {
      setModeAnnouncement('Voice recording is not supported in this browser.')
      return
    }

    try {
      const voiceLang = settings().voiceLang
      const onTranscript = props.onMessageChange
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      audioChunks = []
      const recorder = new MediaRecorder(stream)
      mediaRecorder = recorder

      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) audioChunks = [...audioChunks, event.data]
      }
      recorder.onstop = () => {
        const audio = new Blob(audioChunks, { type: recorder.mimeType || 'audio/webm' })
        audioChunks = []
        stream.getTracks().forEach((track) => track.stop())
        mediaRecorder = undefined
        setVoiceRecording(false)

        void transcribeAudioBlob(audio, voiceLang)
          .then((transcript) => {
            if (transcript) onTranscript(transcript)
          })
          .catch(() => {
            setModeAnnouncement('Voice transcription is unavailable.')
          })
      }
      recorder.onerror = () => {
        stream.getTracks().forEach((track) => track.stop())
        mediaRecorder = undefined
        setVoiceRecording(false)
      }

      recorder.start()
      setVoiceRecording(true)
    } catch {
      setVoiceRecording(false)
      setModeAnnouncement('Microphone access was not available.')
    }
  }

  const clearFiles = () => {
    files().forEach(revokeFilePreviewUrl)
    setFiles([])
  }

  const addScreenshotFile = async () => {
    if (!navigator.mediaDevices?.getDisplayMedia) return

    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({ video: true })
      const video = document.createElement('video')
      video.srcObject = stream
      await new Promise<void>((resolve) => {
        video.onloadedmetadata = () => resolve()
      })
      await video.play()
      await new Promise((resolve) => window.setTimeout(resolve, 100))

      const canvas = document.createElement('canvas')
      canvas.width = video.videoWidth
      canvas.height = video.videoHeight
      canvas.getContext('2d')?.drawImage(video, 0, 0)
      stream.getTracks().forEach((track) => track.stop())

      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      if (!blob) return

      const file = new File([blob], `screenshot-${Date.now()}.png`, { type: 'image/png' })
      addFiles([file])
    } catch {
      // Browser throws when the user cancels capture; the composer stays unchanged.
    }
  }

  const closeSecondaryPanels = () => {
    batch(() => {
      setHistoryOpen(false)
      setSettingsOpen(false)
      setSuggestionsOpen(false)
      setAutocomplete(null)
    })
  }

  let historyRequestSeq = 0
  const refreshChatHistory = async () => {
    const requestSeq = ++historyRequestSeq
    setHistoryLoading(true)
    setHistoryError(null)
    try {
      const sessions = await listChatThreads()
      if (requestSeq === historyRequestSeq) setHistoryThreads(sessions)
    } catch {
      if (requestSeq === historyRequestSeq) setHistoryError('Could not load conversations.')
    } finally {
      if (requestSeq === historyRequestSeq) setHistoryLoading(false)
    }
  }

  const openChatThread = (threadId: string) => {
    selectChatThread(threadId)
    setHistoryOpen(false)
    if (window.location.pathname !== '/chat') navigateToChat(navigate)
  }

  const resetComposerDraft = () => {
    batch(() => {
      props.onMessageChange('')
      clearFiles()
      setActiveActions([])
      closeSecondaryPanels()
      setModelOpen(false)
      setEntities([])
    })
  }

  const openHistoryPanel = () => {
    const nextOpen = !historyOpen()
    if (nextOpen) {
      const rect = historyTriggerRef?.getBoundingClientRect()
      if (rect) {
        setHistoryPanelPosition({
          bottom: window.innerHeight - rect.top + 8,
          maxHeight: Math.max(180, Math.min(400, rect.top - 16)),
          right: window.innerWidth - rect.right,
        })
      }
      void refreshChatHistory()
    }

    setHistoryOpen(nextOpen)
    setSettingsOpen(false)
    setModelOpen(false)
    setSuggestionsOpen(false)
    setAutocomplete(null)
  }

  const openSettingsPanel = () => {
    if (!settingsOpen()) {
      const rect = settingsTriggerRef?.getBoundingClientRect()
      if (rect) {
        const width = 256
        const gap = 8
        const left = Math.min(rect.right + gap, window.innerWidth - width - gap)
        setSettingsPanelPosition({
          left: Math.max(gap, left),
          maxHeight: Math.max(220, Math.min(460, window.innerHeight - rect.top - 16)),
          top: rect.top,
        })
      }
    }

    setSettingsOpen((current) => !current)
    setHistoryOpen(false)
    setModelOpen(false)
    setSuggestionsOpen(false)
    setAutocomplete(null)
  }

  const updateAutocomplete = (text: string, position: number) => {
    if (autocompleteTimer) {
      window.clearTimeout(autocompleteTimer)
      autocompleteTimer = undefined
    }
    autocompleteController?.abort()
    autocompleteController = undefined

    const trigger = detectTrigger(text, position)
    setAutocompleteIndex(0)

    if (!trigger) {
      setAutocomplete(null)
      return
    }

    if (trigger.type === 'date') {
      const items = getUpcomingDates(trigger.dayIndex)
      setAutocomplete(items.length > 0
        ? { category: 'Schedule', items, triggerStart: trigger.start, triggerLen: trigger.rawLen }
        : null)
      return
    }

    if (trigger.type === 'slash') {
      ensureActionsLoaded()
      openActionMenu(trigger.query, trigger.start, trigger.rawLen)
      return
    }

    if (trigger.query.length === 0) {
      setAutocomplete(null)
      return
    }

    const controller = new AbortController()
    autocompleteController = controller
    autocompleteTimer = window.setTimeout(() => {
      searchNavbar(trigger.query, controller.signal)
        .then((payload) => {
          const items = payload.results.slice(0, trigger.type === 'person' ? 6 : 5).map((result) => ({
            id: result.id,
            icon: trigger.type === 'person' ? User : FileText,
            label: result.label,
            meta: trigger.type === 'person' ? 'person' as const : 'file' as const,
          }))
          setAutocomplete(items.length > 0
            ? {
              category: trigger.type === 'person' ? 'People' : 'Documents',
              items,
              triggerStart: trigger.start,
              triggerLen: trigger.rawLen,
            }
            : null)
        })
        .catch(() => {
          if (!controller.signal.aborted) setAutocomplete(null)
        })
    }, 180)
  }

  const handleMessageInput = (value: string, cursorPosition: number) => {
    if (modeAnnouncement()) {
      setModeAnnouncement(null)
      if (modeAnnouncementTimer) {
        window.clearTimeout(modeAnnouncementTimer)
        modeAnnouncementTimer = undefined
      }
    }

    props.onMessageChange(value)
    updateAutocomplete(value, cursorPosition)
    setEntities((current) =>
      current.filter((entity) => {
        const end = entity.start + entity.text.length
        return end <= value.length && value.slice(entity.start, end) === entity.text
      }),
    )
  }

  const autocompleteTriggerParts = (state: AutocompleteState) => ({
    after: props.message.slice(state.triggerStart + state.triggerLen),
    before: props.message.slice(0, state.triggerStart),
  })

  const clearAutocompleteTrigger = (state: AutocompleteState) => {
    const parts = autocompleteTriggerParts(state)
    props.onMessageChange(parts.before + parts.after)
    setAutocomplete(null)
    return parts.before.length
  }

  const selectSpecializedAutocomplete = (state: AutocompleteState, action: SpecializedAction) => {
    const caret = clearAutocompleteTrigger(state)
    activateSpecialized(action)
    focusTextareaAtNextFrame(caret)
  }

  const selectFileAutocompleteCommand = (state: AutocompleteState) => {
    pendingFilePosition = state.triggerStart
    clearAutocompleteTrigger(state)
    openFileDialog()
  }

  const selectImageAutocompleteCommand = (state: AutocompleteState) => {
    const parts = autocompleteTriggerParts(state)
    const prefix = '/image '
    props.onMessageChange(`${parts.before}${prefix}${parts.after}`)
    props.onImageModeChange?.(true)
    setAutocomplete(null)
    focusTextareaAtNextFrame(parts.before.length + prefix.length)
  }

  const insertAutocompleteItem = (state: AutocompleteState, item: AutocompleteItem) => {
    const parts = autocompleteTriggerParts(state)
    const inserted = item.meta === 'person' ? `@${item.label}` : item.label
    props.onMessageChange(parts.before + inserted + parts.after)

    const meta = item.meta
    if (meta === 'date' || meta === 'person' || meta === 'file') {
      setEntities((current) => [...current, { kind: meta, start: state.triggerStart, text: inserted }])
    }

    setAutocomplete(null)
    focusTextareaAtNextFrame(state.triggerStart + inserted.length)
  }

  const applyAutocompleteSelection = (item: AutocompleteItem) => {
    const state = autocomplete()
    if (!state) return

    if (item.specialized) {
      selectSpecializedAutocomplete(state, item.specialized)
      return
    }

    const command = slashCommands.find((candidate) => candidate.id === item.id)
    if (command?.action === 'file') {
      selectFileAutocompleteCommand(state)
      return
    }
    if (command?.action === 'image') {
      selectImageAutocompleteCommand(state)
      return
    }

    insertAutocompleteItem(state, item)
  }

  const handleAutocompleteKeyDown = (
    event: KeyboardEvent & { currentTarget: HTMLTextAreaElement },
    state: AutocompleteState,
  ) => {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        setAutocompleteIndex((current) => Math.min(current + 1, state.items.length - 1))
        return true
      case 'ArrowUp':
        event.preventDefault()
        setAutocompleteIndex((current) => Math.max(current - 1, 0))
        return true
      case 'Enter':
      case 'Tab': {
        event.preventDefault()
        const item = state.items[autocompleteIndex()]
        if (item) applyAutocompleteSelection(item)
        return true
      }
      case 'Escape':
        event.preventDefault()
        setAutocomplete(null)
        return true
      default:
        return false
    }
  }

  const handleComposerKeyDown = (event: KeyboardEvent & { currentTarget: HTMLTextAreaElement }) => {
    const state = autocomplete()
    if (state && handleAutocompleteKeyDown(event, state)) return

    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      void submitComposer()
    }
  }

  const handleResponseMode = (mode: ResponseMode) => {
    if (mode === responseMode()) return

    const nextMode = responseModes.find((candidate) => candidate.id === mode)
    setResponseMode(mode)
    if (nextMode) {
      setModeAnnouncement(nextMode.announcement)
      if (modeAnnouncementTimer) window.clearTimeout(modeAnnouncementTimer)
      modeAnnouncementTimer = window.setTimeout(() => setModeAnnouncement(null), 3000)
    }
  }

  const createSubmissionSnapshot = () => {
    const now = new Date()
    const body = props.message.trim()
    const submittedText = body || 'Vedlegg sendt til Velion.'
    return {
      actions: activeActions(),
      body,
      files: files(),
      now,
      submittedText,
    }
  }

  const submitComposer = async () => {
    if (autocomplete() || !hasContent() || voiceMode() || voiceRecording() || props.submitting) return

    const snapshot = createSubmissionSnapshot()
    const payload = createComposerSubmitPayload({
      actions: snapshot.actions,
      browseWeb: browseWeb(),
      deepSearch: deepSearch(),
      files: snapshot.files,
      imageMode: imageMode(),
      model: selectedModel(),
      responseMode: responseMode(),
      text: snapshot.submittedText,
      trimmedMessage: snapshot.body,
    })

    setTurns((current) => [
      createComposerTurn({
        body: snapshot.submittedText,
        browseWeb: browseWeb(),
        deepSearch: deepSearch(),
        files: snapshot.files,
        model: selectedModelLabel(),
        now: snapshot.now,
        responseMode: responseMode(),
      }),
      ...current,
    ].slice(0, 6))

    if (props.onSubmit) {
      await props.onSubmit(payload)
      resetComposerDraft()
      return
    }

    await writePendingChatLaunch(payload)
    resetComposerDraft()
    props.onLaunchStart?.()
    // Navigate immediately inside a View Transition: the composer carries
    // `view-transition-name: velion-composer`, so it morphs (travels + resizes) in
    // place into the chat page's composer instead of fading out first and
    // crossfading the whole route. Reduced-motion users get a plain navigate.
    navigateToChat(navigate)
  }

  onCleanup(() => {
    mediaRecorder?.stream.getTracks().forEach((track) => track.stop())
    if (autocompleteTimer) window.clearTimeout(autocompleteTimer)
    autocompleteController?.abort()
    if (modeAnnouncementTimer) window.clearTimeout(modeAnnouncementTimer)
    files().forEach(revokeFilePreviewUrl)
  })

  return (
    <div
      ref={(element) => { composerRootRef = element }}
      class={dashboardComposerRootClass({ dragActive: dragActive() })}
      onDragOver={(event) => {
        if (dragEventHasFiles(event)) {
          event.preventDefault()
          setDragActive(true)
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setDragActive(false)
        }
      }}
      onDrop={(event) => {
        const dropped = droppedFiles(event)
        if (dropped.length > 0) {
          event.preventDefault()
          addFiles(dropped)
        }
        setDragActive(false)
      }}
      onPaste={(event) => {
        const images = pastedImageFiles(event)
        if (images.length > 0) {
          event.preventDefault()
          addFiles(images)
        }
      }}
    >
      <Show when={dragActive()}>
        <div class="dashboard-composer-drop-overlay">
          Slipp for å legge ved
        </div>
      </Show>

      <div class="dashboard-composer-controls">
        <div class="dashboard-composer-controls__left">
          <div class="dashboard-composer-model-wrap">
            <button
              type="button"
              aria-expanded={modelOpen()}
              aria-label="Velg AI-modell"
              title="Velg AI-modell"
              onClick={() => {
                setModelOpen((current) => !current)
                closeSecondaryPanels()
              }}
              class="dashboard-composer-model-button velion-composer-control"
            >
              <Zap class="size-4 dashboard-composer-model-button__zap" />
              <span>{selectedModelLabel()}</span>
              <ChevronDown
                class={composerChevronClass(modelOpen())}
              />
            </button>

            <Show when={modelOpen()}>
              <div class="dashboard-composer-model-menu velion-popover">
                <div class="dashboard-composer-model-group">
                  <p class="dashboard-composer-model-group__label">Velion</p>
                  <For each={VELION_MODES}>
                    {(mode) => (
                      <button
                        type="button"
                        title={`${mode.label} — ${mode.description}`}
                        onClick={() => selectModel(mode.id)}
                        classList={{ 'dashboard-composer-model-menu__item--active': selectedModel() === mode.id }}
                      >
                        <span>
                          <span><Zap class="size-3" /></span>
                          <span>{mode.label}</span>
                        </span>
                        <span class="dashboard-composer-model-menu__right">
                          <Show
                            when={mode.badge === 'premium'}
                            fallback={
                              <span class="dashboard-composer-model-badge dashboard-composer-model-badge--cheap">Rimelig</span>
                            }
                          >
                            <span
                              class="dashboard-composer-model-badge dashboard-composer-model-badge--premium"
                              title="Premium — beste svar, dyrere"
                            >
                              $$
                            </span>
                          </Show>
                          <Show when={selectedModel() === mode.id}>
                            <Check class="size-4" />
                          </Show>
                        </span>
                      </button>
                    )}
                  </For>
                </div>

                <For each={chatModelGroups()}>
                  {(group) => (
                    <div class="dashboard-composer-model-group">
                      <p class="dashboard-composer-model-group__label">{group.label}</p>
                      <For each={group.models}>
                        {(model) => (
                          <button
                            type="button"
                            title={`Bruk ${model.name}`}
                            onClick={() => selectModel(model.id)}
                            classList={{ 'dashboard-composer-model-menu__item--active': selectedModel() === model.id }}
                          >
                            <span>
                              <span><Zap class="size-3" /></span>
                              <span>{model.name}</span>
                            </span>
                            <span class="dashboard-composer-model-menu__right">
                              <Show when={isExpensiveModel(model)}>
                                <span
                                  class="dashboard-composer-model-badge dashboard-composer-model-badge--premium"
                                  title="Dyrere modell — velg bevisst"
                                >
                                  $$
                                </span>
                              </Show>
                              <Show when={selectedModel() === model.id}>
                                <Check class="size-4" />
                              </Show>
                            </span>
                          </button>
                        )}
                      </For>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </div>

          <A
            href="/agents"
            title="Opprett agent"
            class="dashboard-composer-agent-button velion-composer-control"
            onClick={() => {
              props.onMessageChange('Opprett en agent som håndterer kundesamtaler med kunnskapsbase, tone og eskaleringer.')
              setSuggestionsOpen(true)
              setModelOpen(false)
            }}
          >
            <Sparkles class="dashboard-composer-agent-button__icon" />
            <span>Create agent</span>
          </A>
        </div>

        <div class="dashboard-composer-controls__right">
          <Show when={props.onPlanModeChange}>
            <ComposerIconButton
              active={planMode()}
              label="Planmodus — agenten planlegger og ber om godkjenning før risikable verktøy"
              onClick={() => props.onPlanModeChange?.(!planMode())}
              variant="chip"
            >
              <WandSparkles class="size-3.5" />
            </ComposerIconButton>
          </Show>
          <span ref={(element) => { historyTriggerRef = element }}>
            <ComposerIconButton active={historyOpen()} label="Historikk" onClick={openHistoryPanel} variant="chip">
              <Clock3 class="size-3.5" />
            </ComposerIconButton>
          </span>
          <span ref={(element) => { settingsTriggerRef = element }}>
            <ComposerIconButton active={settingsOpen()} label="Innstillinger" onClick={openSettingsPanel} variant="chip">
              <SlidersHorizontal class="size-3.5" />
            </ComposerIconButton>
          </span>
        </div>
      </div>

      <div class="dashboard-composer-field-wrap">
        <input
          ref={(element) => { fileInputRef = element }}
          type="file"
          accept="*/*"
          multiple
          class="sr-only"
          aria-label="Add files"
          onChange={(event) => {
            if (event.currentTarget.files) addFiles(event.currentTarget.files)
            event.currentTarget.value = ''
          }}
        />

        <Show when={suggestionsOpen()}>
          <div class="dashboard-composer-suggestions velion-popover velion-popover-up">
            <button type="button" onClick={() => props.onMessageChange('Finn de viktigste kundesakene fra siste uke.')}>Finn de viktigste kundesakene fra siste uke.</button>
            <button type="button" onClick={() => props.onMessageChange('Lag et kort svarutkast med kildehenvisninger.')}>Lag et kort svarutkast med kildehenvisninger.</button>
            <button type="button" onClick={() => props.onMessageChange('Oppsummer kunnskapsbasen og pek på mangler.')}>Oppsummer kunnskapsbasen og pek på mangler.</button>
          </div>
        </Show>

        <Show when={autocomplete()}>
          {(state) => (
            <div class="dashboard-composer-autocomplete">
              <AutocompleteDropdown
                category={state().category}
                items={state().items}
                selectedIndex={autocompleteIndex()}
                onHover={setAutocompleteIndex}
                onSelect={applyAutocompleteSelection}
              />
            </div>
          )}
        </Show>

        <form
          class="velion-dashboard-composer-card dashboard-composer-card velion-composer-shell"
          onSubmit={(event) => {
            event.preventDefault()
            void submitComposer()
          }}
        >
          <AttachmentPreview attachments={files()} onEnhance={enhanceAttachments} onRemove={removeFile} />

          <Show when={activeActions().length > 0}>
            <div class="dashboard-composer-actions-bar">
              <For each={activeActions()}>
                {(action) => (
                  <span class={cn('dashboard-composer-action-chip', `dashboard-composer-action-chip--${action.kind}`)}>
                    <Dynamic component={iconForKind(action.kind)} class="size-3" />
                    <span>{action.name}</span>
                    <button
                      type="button"
                      aria-label={`Remove ${action.name}`}
                      title={`Remove ${action.name}`}
                      onClick={() => removeActiveAction(action)}
                    >
                      <X class="size-2.5" />
                    </button>
                  </span>
                )}
              </For>
            </div>
          </Show>

          <div class="dashboard-composer-textarea-wrap">
            <Show when={hasEntityOverlay()}>
              <div class="dashboard-composer-entity-overlay" aria-hidden="true">
                <EntityOverlay entities={entities()} message={props.message} />
              </div>
            </Show>
            <Show when={currentModeAnnouncement()}>
              <div class="dashboard-composer-mode-announcement" aria-hidden="true">
                <SplitText text={currentModeAnnouncement()} />
              </div>
            </Show>
            <label class="sr-only" for="velion-dashboard-input">Message Velion</label>
            <textarea
              ref={(element) => { textareaRef = element }}
              id="velion-dashboard-input"
              aria-label="Message Velion"
              class="velion-dashboard-textarea"
              aria-busy={voiceRecording()}
              placeholder={textareaPlaceholder(modeAnnouncement())}
              rows="3"
              style={textareaOverlayStyle(hasEntityOverlay())}
              value={props.message}
              onInput={(event) => handleMessageInput(event.currentTarget.value, textareaCursorPosition(event.currentTarget))}
              onKeyDown={handleComposerKeyDown}
              disabled={textareaLocked()}
            />
            <Show when={showTextareaExpand()}>
              <button
                type="button"
                class="dashboard-composer-expand-btn"
                aria-label={textareaExpandLabel(textareaExpanded())}
                title={textareaExpandTitle(textareaExpanded())}
                onClick={() => setTextareaExpanded((prev) => !prev)}
              >
                <Show when={textareaExpanded()} fallback={<Maximize2 size={11} strokeWidth={2} />}>
                  <Minimize2 size={11} strokeWidth={2} />
                </Show>
              </button>
            </Show>
          </div>

          <Show when={voiceRecording()}>
            <div class="dashboard-composer-loading-bar">
              <span />
            </div>
          </Show>

          <div class="dashboard-composer-toolbar">
            <div class="dashboard-composer-toolbar__left">
              <button type="button" class="dashboard-composer-attach" aria-label="Add files" title="Add files" onClick={openFileDialog}>
                <span><CirclePlus class="size-4" /></span>
                <span>add files</span>
              </button>
              <div class="dashboard-composer-toolbar__divider" />
              <ComposerIconButton active={suggestionsOpen()} label="Suggestions" onClick={() => setSuggestionsOpen((current) => !current)} variant="toolbar">
                <Lightbulb class="size-4" />
              </ComposerIconButton>
              <ComposerIconButton active={deepSearch()} label="Deep search" onClick={() => setDeepSearch((current) => !current)} variant="toolbar">
                <Telescope class="size-4" />
              </ComposerIconButton>
              <button
                type="button"
                aria-pressed={browseWeb()}
                aria-label="Browse web"
                title="Browse web"
                onClick={() => setBrowseWeb((current) => !current)}
                class={composerWebButtonClass(browseWeb())}
              >
                <Globe2 class="size-4" />
                Search
              </button>
              <Show when={props.onImageModeChange}>
                <button
                  type="button"
                  aria-pressed={imageMode()}
                  aria-label="Generer bilde"
                  title="Generer bilde"
                  onClick={() => props.onImageModeChange?.(!imageMode())}
                  class={composerImageButtonClass(imageMode())}
                >
                  <ImagePlus class="size-4" />
                  Bilde
                </button>
              </Show>
            </div>

            <div class="dashboard-composer-toolbar__right">
              <div class="dashboard-composer-response-group">
                <For each={responseModes}>
                  {(mode) => (
                    <ComposerIconButton
                      active={responseMode() === mode.id}
                      label={mode.label}
                      onClick={() => handleResponseMode(mode.id)}
                      variant="toolbar"
                    >
                      <Dynamic component={mode.icon} class="size-4" />
                    </ComposerIconButton>
                  )}
                </For>
              </div>
              <ComposerIconButton active={voiceMode()} label="Voice mode" onClick={() => setVoiceMode(true)} variant="toolbar">
                <AudioWaveform class="size-4" />
              </ComposerIconButton>
              <ComposerIconButton active={voiceRecording()} label="Voice input" onClick={() => void toggleRecording()} variant="toolbar">
                <Mic class="size-4" />
              </ComposerIconButton>
              <Show
                when={stopButtonVisible()}
                fallback={
                  <button
                    type="submit"
                    disabled={!submitEnabled()}
                    aria-label="Send message"
                    title="Send message"
                    class={composerSubmitButtonClass(submitEnabled())}
                  >
                    <ArrowUp class="size-4" />
                  </button>
                }
              >
                <button
                  type="button"
                  aria-label="Stop response"
                  title="Stop response"
                  onClick={() => props.onStop?.()}
                  class="dashboard-composer-submit-button dashboard-composer-submit-button--active"
                >
                  <Square class="size-3.5" />
                </button>
              </Show>
            </div>
          </div>
        </form>
      </div>

      <Show when={activeTurnReceipt()}>
        {(turn) => <TurnReceipt turn={turn()} />}
      </Show>

      {/* Portal floating panels/overlays to <body>: they use position:fixed and
          the composer subtree carries an identity transform (velionPanelIn) that
          would otherwise become their containing block and mis-place them. */}
      <Show when={historyOpen()}>
        <Portal>
          <HistoryPanel
            error={historyError()}
            loading={historyLoading()}
            position={historyPanelPosition()}
            threads={historyThreads()}
            turns={turns()}
            onClose={() => setHistoryOpen(false)}
            onThreadSelect={openChatThread}
          />
        </Portal>
      </Show>

      <Show when={settingsOpen()}>
        <Portal>
          <SettingsPanel
            position={settingsPanelPosition()}
            settings={settings()}
            onAddFiles={() => {
              setSettingsOpen(false)
              openFileDialog()
            }}
            onScreenshot={() => {
              setSettingsOpen(false)
              void addScreenshotFile()
            }}
            onSettingsChange={setSettings}
          />
        </Portal>
      </Show>
      <Show when={voiceMode()}>
        <Portal>
          <RealtimeVoiceModal
            language={settings().voiceLang}
            model={selectedModelLabel()}
            onClose={() => setVoiceMode(false)}
            onListeningChange={setVoiceRecording}
            onTranscript={(text) => {
              const trimmed = props.message.trim()
              props.onMessageChange(trimmed ? `${trimmed} ${text}` : text)
            }}
          />
        </Portal>
      </Show>
    </div>
  )
}

function iconForKind(kind: SpecializedActionKind): Component<LucideProps> {
  switch (kind) {
    case 'skill':
      return WandSparkles
    case 'capability':
      return Blocks
    case 'connector':
      return Briefcase
    case 'builtin':
      return Sparkles
  }
}

function iconForAction(action: SpecializedAction): Component<LucideProps> {
  if (action.kind === 'builtin') {
    if (action.builtin === 'file') return Upload
    if (action.builtin === 'image') return ImagePlus
    if (action.builtin === 'web_search') return Globe2
    return Sparkles
  }
  return iconForKind(action.kind)
}

function getComposerTools(input: {
  browseWeb: boolean
  deepSearch: boolean
  imageMode: boolean
  message: string
  responseMode: ResponseMode
}): DashboardComposerSubmitPayload['tools'] {
  const tools: DashboardComposerSubmitPayload['tools'] = []
  if (input.browseWeb) tools.push('search')
  if (input.responseMode === 'deep') tools.push('reason')
  if (input.deepSearch) tools.push('research')
  if (input.imageMode || input.message.trimStart().toLowerCase().startsWith('/image ')) tools.push('image')
  return [...new Set(tools)]
}

function createComposerSubmitPayload(input: {
  actions: ComposerActiveAction[]
  browseWeb: boolean
  deepSearch: boolean
  files: ComposerFile[]
  imageMode: boolean
  model: string
  responseMode: ResponseMode
  text: string
  trimmedMessage: string
}): DashboardComposerSubmitPayload {
  return {
    actions: input.actions,
    attachments: input.files.map((file) => ({
      id: file.id,
      name: file.name,
      size: file.size,
      type: file.type || 'application/octet-stream',
      url: file.url,
    })),
    model: input.model || undefined,
    text: input.text,
    tools: getComposerTools({
      browseWeb: input.browseWeb,
      deepSearch: input.deepSearch,
      imageMode: input.imageMode,
      message: input.trimmedMessage,
      responseMode: input.responseMode,
    }),
  }
}

function createComposerTurn(input: {
  body: string
  browseWeb: boolean
  deepSearch: boolean
  files: ComposerFile[]
  model: string
  now: Date
  responseMode: ResponseMode
}): ComposerTurn {
  return {
    id: `turn-${input.now.getTime()}`,
    body: input.body,
    browseWeb: input.browseWeb,
    createdAt: formatComposerTurnTime(input.now),
    createdAtIso: input.now.toISOString(),
    deepSearch: input.deepSearch,
    files: input.files.map((file) => file.name),
    model: input.model,
    responseMode: input.responseMode,
  }
}

function detectTrigger(text: string, position: number): TriggerContext | null {
  const before = text.slice(0, position)
  const personMatch = before.match(/@(\w*)$/)
  if (personMatch) {
    const query = personMatch[1] ?? ''
    return {
      type: 'person',
      query: query.toLowerCase(),
      start: position - personMatch[0].length,
      rawLen: personMatch[0].length,
    }
  }

  const slashMatch = before.match(/\/(\w*)$/)
  if (slashMatch) {
    const query = slashMatch[1] ?? ''
    return {
      type: 'slash',
      query: query.toLowerCase(),
      start: position - slashMatch[0].length,
      rawLen: slashMatch[0].length,
    }
  }

  const wordMatch = before.match(/\b([A-Za-z]{3,})$/)
  if (!wordMatch) return null
  const word = wordMatch[1] ?? ''
  const day = dayEntries.find((entry) => entry.name.toLowerCase().startsWith(word.toLowerCase()))
  if (!day) return null
  return {
    type: 'date',
    dayIndex: day.dayIndex,
    start: position - word.length,
    rawLen: word.length,
  }
}

function getUpcomingDates(dayIndex: number): AutocompleteItem[] {
  const items: AutocompleteItem[] = []
  const date = new Date()

  while (items.length < 2) {
    date.setDate(date.getDate() + 1)
    if (date.getDay() === dayIndex) {
      const label = date.toLocaleDateString('en-US', {
        day: 'numeric',
        month: 'short',
        weekday: 'short',
      })
      items.push({ id: `date-${label}`, icon: Calendar, label, meta: 'date' })
    }
  }

  return items
}

function EntityOverlay(props: {
  entities: EntityToken[]
  message: string
}) {
  const parts = () => {
    const output: JSX.Element[] = []
    let position = 0
    const validEntities = props.entities
      .filter((entity) => entity.start >= 0 && entity.start + entity.text.length <= props.message.length && props.message.slice(entity.start, entity.start + entity.text.length) === entity.text)
      .sort((a, b) => a.start - b.start)

    validEntities.forEach((entity) => {
      if (entity.start > position) {
        output.push(<span class="dashboard-composer-entity-text">{props.message.slice(position, entity.start)}</span>)
      }
      output.push(
        <span class={cn('dashboard-composer-entity-token', `dashboard-composer-entity-token--${entity.kind}`)}>
          <Show when={entity.kind === 'file'}>
            <span />
          </Show>
          {entity.text}
        </span>,
      )
      position = entity.start + entity.text.length
    })

    if (position < props.message.length) {
      output.push(<span class="dashboard-composer-entity-text">{props.message.slice(position)}</span>)
    }

    return output.length > 0 ? output : <span>{props.message || '\u200b'}</span>
  }

  return <>{parts()}</>
}

function AutocompleteDropdown(props: {
  category: string
  items: AutocompleteItem[]
  onHover: (index: number) => void
  onSelect: (item: AutocompleteItem) => void
  selectedIndex: number
}) {
  return (
    <div class="dashboard-composer-autocomplete-menu velion-popover velion-popover-up" data-composer-floating-panel="true">
      <div class="dashboard-composer-autocomplete-menu__category">{props.category}</div>
      <For each={props.items}>
        {(item, index) => (
          <button
            type="button"
            onMouseDown={(event) => {
              event.preventDefault()
              props.onSelect(item)
            }}
            onMouseEnter={() => props.onHover(index())}
            classList={{ 'dashboard-composer-autocomplete-menu__item--active': index() === props.selectedIndex }}
          >
            <Dynamic component={item.icon} class="size-4" />
            <span>{item.label}</span>
          </button>
        )}
      </For>
    </div>
  )
}

type SpeechResultLike = { readonly 0: { transcript: string }; isFinal: boolean }
type SpeechEventLike = { resultIndex: number; results: ArrayLike<SpeechResultLike> }
type SpeechRecognitionLike = {
  continuous: boolean
  interimResults: boolean
  lang: string
  onend: (() => void) | null
  onerror: (() => void) | null
  onresult: ((event: SpeechEventLike) => void) | null
  start: () => void
  stop: () => void
}
type SpeechRecognitionCtor = new () => SpeechRecognitionLike

function getSpeechRecognitionCtor(): SpeechRecognitionCtor | null {
  const win = window as unknown as {
    SpeechRecognition?: SpeechRecognitionCtor
    webkitSpeechRecognition?: SpeechRecognitionCtor
  }
  return win.SpeechRecognition ?? win.webkitSpeechRecognition ?? null
}

function RealtimeVoiceModal(props: {
  language: string
  model: string
  onClose: () => void
  onListeningChange: (listening: boolean) => void
  onTranscript: (text: string) => void
}) {
  let recognition: SpeechRecognitionLike | null = null
  const [interim, setInterim] = createSignal('')
  const [listening, setListening] = createSignal(false)
  const [transcript, setTranscript] = createSignal('')
  const supported = () => getSpeechRecognitionCtor() !== null
  const liveText = () => `${transcript()}${interim() ? ` ${interim()}` : ''}`.trim()

  const stop = () => {
    recognition?.stop()
    recognition = null
    setListening(false)
    props.onListeningChange(false)
    setInterim('')
  }

  const startListening = () => {
    const Ctor = getSpeechRecognitionCtor()
    if (!Ctor) return
    const nextRecognition = new Ctor()
    nextRecognition.lang = props.language || 'nb-NO'
    nextRecognition.continuous = true
    nextRecognition.interimResults = true
    nextRecognition.onresult = (event) => {
      let finalChunk = ''
      let interimChunk = ''
      for (let index = event.resultIndex; index < event.results.length; index += 1) {
        const result = event.results[index]
        if (!result) continue
        const text = result[0]?.transcript ?? ''
        if (result.isFinal) finalChunk += text
        else interimChunk += text
      }
      if (finalChunk) {
        setTranscript((current) => current ? `${current} ${finalChunk.trim()}` : finalChunk.trim())
      }
      setInterim(interimChunk)
    }
    nextRecognition.onend = () => {
      setListening(false)
      props.onListeningChange(false)
    }
    nextRecognition.onerror = () => {
      setListening(false)
      props.onListeningChange(false)
    }
    recognition = nextRecognition
    setListening(true)
    props.onListeningChange(true)
    nextRecognition.start()
  }

  const close = () => {
    stop()
    setTranscript('')
    props.onClose()
  }

  const insert = () => {
    const text = liveText()
    if (text) props.onTranscript(text)
    close()
  }

  onCleanup(() => stop())

  return (
    <dialog open class="realtime-voice-modal" aria-label="Voice mode" data-dashboard-modal="true">
      <button type="button" aria-label="Dismiss voice backdrop" class="realtime-voice-modal__scrim" onClick={close} />
      <div class="realtime-voice-modal__panel velion-panel-in">
        <div class="realtime-voice-modal__header">
          <span>
            <AudioWaveform class="size-5" />
          </span>
          <div>
            <p>Stemmemodus</p>
            <small>{props.model} · {props.language}</small>
          </div>
          <button type="button" onClick={close} aria-label="Lukk stemmemodus">
            <X class="size-4" />
          </button>
        </div>

        <div class="realtime-voice-modal__body">
          <button
            type="button"
            onClick={() => listening() ? stop() : startListening()}
            disabled={!supported()}
            class={cn('realtime-voice-modal__mic', listening() ? 'realtime-voice-modal__mic--listening velion-voice-pulse' : '')}
            aria-label={listening() ? 'Stop dictation' : 'Start dictation'}
          >
            <Mic class="size-6" />
          </button>
          <Show
            when={liveText()}
            fallback={
              <>
                <p>{listening() ? 'Lytter …' : supported() ? 'Klar for diktering' : 'Stemme støttes ikke i denne nettleseren'}</p>
                <small>{supported() ? 'Snakk fritt. Teksten settes inn i meldingen.' : 'Prøv Chrome/Edge, eller skriv meldingen i stedet.'}</small>
              </>
            }
          >
            <p class="realtime-voice-modal__transcript">
              {transcript()}
              <Show when={interim()}>
                {(text) => <span> {text()}</span>}
              </Show>
            </p>
          </Show>
        </div>

        <div class="realtime-voice-modal__actions">
          <button type="button" onClick={() => listening() ? stop() : startListening()} disabled={!supported()}>
            {listening() ? 'Stopp' : 'Start diktering'}
          </button>
          <button type="button" onClick={insert} disabled={!liveText()}>
            Sett inn
          </button>
        </div>
      </div>
    </dialog>
  )
}

function prefersReducedMotion() {
  return typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function navigateToChat(navigate: ReturnType<typeof useNavigate>) {
  const doc = document as Document & {
    startViewTransition?: (callback: () => void) => { finished: Promise<void> }
  }

  if (!prefersReducedMotion() && typeof doc.startViewTransition === 'function') {
    doc.startViewTransition(() => navigate('/chat'))
    return
  }

  navigate('/chat')
}

function createFilePreviewUrl(file: File) {
  if (typeof URL === 'undefined' || typeof URL.createObjectURL !== 'function') {
    return ''
  }

  return URL.createObjectURL(file)
}

function revokeFilePreviewUrl(file: ComposerFile) {
  if (!file.url || typeof URL === 'undefined' || typeof URL.revokeObjectURL !== 'function') {
    return
  }

  URL.revokeObjectURL(file.url)
}

function formatFileSize(size: number) {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function AttachmentPreview(props: {
  attachments: ComposerFile[]
  onEnhance: () => void
  onRemove: (id: string) => void
}) {
  return (
    <Show when={props.attachments.length > 0}>
      <div class="velion-attachment-preview dashboard-composer-attachments">
        <div class="dashboard-composer-attachments__list">
          <For each={props.attachments}>
            {(attachment) => (
              <div class="dashboard-composer-attachment">
                <Show
                  when={attachment.type.startsWith('image/') && attachment.url}
                  fallback={
                    <div class="dashboard-composer-attachment__file">
                      <FileText class="size-5" />
                      <span>{attachment.name}</span>
                      <small>{formatFileSize(attachment.size)}</small>
                    </div>
                  }
                >
                  <img src={attachment.url} alt={attachment.name} />
                </Show>
                <button
                  type="button"
                  onClick={() => props.onRemove(attachment.id)}
                  class="dashboard-composer-attachment__remove"
                  aria-label={`Remove ${attachment.name}`}
                  title={`Remove ${attachment.name}`}
                >
                  <X class="size-2.5" />
                </button>
              </div>
            )}
          </For>
        </div>
        <button
          type="button"
          onClick={() => props.onEnhance()}
          class="dashboard-composer-attachments__enhance"
          aria-label="AI enhance"
          title="AI enhance"
        >
          <WandSparkles class="size-4" />
        </button>
      </div>
    </Show>
  )
}

function TurnReceipt(props: { turn: ComposerTurn }) {
  return (
    <div class="velion-composer-turn-receipt velion-fade-up">
      <div>
        <span>
          <ArrowUp class="size-3.5" />
        </span>
        <div>
          <p>{props.turn.body}</p>
          <small>
            {props.turn.model} · {props.turn.responseMode}
            <Show when={props.turn.browseWeb}> · web</Show>
            <Show when={props.turn.deepSearch}> · deep search</Show>
          </small>
          <Show when={props.turn.files.length > 0}>
            <small>{props.turn.files.length} vedlegg lagt til.</small>
          </Show>
        </div>
      </div>
    </div>
  )
}

function HistoryPanel(props: {
  error: string | null
  loading: boolean
  onClose: () => void
  onThreadSelect: (threadId: string) => void
  position: PanelPosition
  threads: ChatThreadSession[]
  turns: ComposerTurn[]
}) {
  const items = createMemo(() => [
    ...props.threads.map(threadToHistoryItem),
    ...props.turns.map(turnToHistoryItem),
  ])
  const groups = createMemo(() => historyGroups(items()))

  return (
    <div
      data-composer-floating-panel="true"
      class="velion-popover velion-popover-up velion-floating-panel velion-floating-panel-sm velion-floating-panel-compact dashboard-composer-floating-panel dashboard-composer-history-panel"
      style={panelPositionStyle(props.position)}
    >
      <div class="dashboard-composer-floating-panel__scroll" style={{ 'max-height': `${props.position.maxHeight}px` }}>
        <Show when={props.loading}>
          <div class="velion-menu-row dashboard-composer-history-row dashboard-composer-history-row--loading">
            <Loader2 class="size-4 shrink-0 animate-spin" strokeWidth={1.7} />
            <span>
              <span class="velion-menu-label">Loading conversations...</span>
              <span class="velion-menu-meta">Signed-in user history</span>
            </span>
          </div>
        </Show>
        <Show when={items().length > 0}>
          <For each={groups()}>
            {(group) => (
              <Show when={group.items.length > 0}>
                <div>
                  <p class="dashboard-composer-history-group-label">{group.label}</p>
                  <For each={group.items}>
                    {(item) => (
                      <button
                        type="button"
                        onClick={() => item.threadId ? props.onThreadSelect(item.threadId) : props.onClose()}
                        class="velion-menu-row dashboard-composer-history-row"
                      >
                        <MessageSquare class="size-4 shrink-0" strokeWidth={1.7} />
                        <span>
                          <span class="velion-menu-label">{item.title || 'Untitled'}</span>
                          <span class="velion-menu-meta">{item.meta}</span>
                        </span>
                        <span class="velion-menu-meta dashboard-composer-history-row__time">
                          {formatHistoryItemTime(item, group.isToday)}
                        </span>
                      </button>
                    )}
                  </For>
                </div>
              </Show>
            )}
          </For>
        </Show>
        <Show when={!props.loading && items().length === 0}>
          <div class="dashboard-composer-history-empty">
            <MessageSquare class="size-5" strokeWidth={1.5} />
            <p>{props.error ?? 'No conversations found for this user.'}</p>
          </div>
        </Show>

        <div class="dashboard-composer-menu-divider" />
        <A href="/chat" onClick={props.onClose} class="velion-menu-row">
          <LayoutGrid class="size-4 shrink-0" strokeWidth={1.7} />
          <span class="velion-menu-label">View all conversations</span>
        </A>
      </div>
    </div>
  )
}

function SettingsPanel(props: {
  onAddFiles: () => void
  onScreenshot: () => void
  onSettingsChange: (settings: ComposerSettings) => void
  position: PanelPosition
  settings: ComposerSettings
}) {
  const [view, setView] = createSignal<SettingsView>('main')
  const currentLangLabel = () => voiceLanguages.find((language) => language.value === props.settings.voiceLang)?.label ?? props.settings.voiceLang
  const updateSetting = <K extends keyof ComposerSettings>(key: K, value: ComposerSettings[K]) => {
    props.onSettingsChange({ ...props.settings, [key]: value })
  }

  return (
    <div
      data-composer-floating-panel="true"
      class="velion-popover velion-popover-side velion-floating-panel velion-floating-panel-xs velion-floating-panel-compact dashboard-composer-floating-panel dashboard-composer-settings-panel"
      style={panelPositionStyle(props.position)}
    >
      <div class="dashboard-composer-floating-panel__scroll" style={{ 'max-height': `${props.position.maxHeight}px` }}>
        <Switch>
          <Match when={view() === 'main'}>
            <div class="velion-settings-view">
              <div class="velion-menu-row">
                <Mic class="size-[17px] shrink-0" strokeWidth={1.7} />
                <span class="velion-menu-label">Voice language</span>
                <select
                  value={props.settings.voiceLang}
                  title={currentLangLabel()}
                  onChange={(event) => updateSetting('voiceLang', event.currentTarget.value)}
                  class="dashboard-composer-settings-select"
                >
                  <For each={voiceLanguages}>
                    {(language) => <option value={language.value}>{language.label}</option>}
                  </For>
                </select>
              </div>

              <For each={toneOptions}>
                {(option) => (
                  <ComposerMenuRow
                    icon={<Dynamic component={option.icon} class="size-[17px]" strokeWidth={1.7} />}
                    label={option.label}
                    onClick={() => updateSetting('tone', option.value)}
                    right={props.settings.tone === option.value ? <Check class="size-3.5" strokeWidth={2.5} /> : <span class="dashboard-composer-menu-spacer" />}
                  />
                )}
              </For>

              <div class="dashboard-composer-menu-divider" />
              <ComposerMenuRow icon={<Paperclip class="size-[17px]" strokeWidth={1.7} />} label="Add files or photos" onClick={props.onAddFiles} />
              <ComposerMenuRow icon={<Camera class="size-[17px]" strokeWidth={1.7} />} label="Take a screenshot" onClick={props.onScreenshot} />
              <ComposerMenuRow
                icon={<FolderPlus class="size-[17px]" strokeWidth={1.7} />}
                label="Add to project"
                onClick={() => setView('projects')}
                right={<ChevronRight class="size-3.5" />}
              />
              <div class="dashboard-composer-menu-divider" />
              <ComposerMenuRow
                icon={<Blocks class="size-[17px]" strokeWidth={1.7} />}
                label="Skills"
                onClick={() => setView('skills')}
                right={<ChevronRight class="size-3.5" />}
              />
              <ComposerMenuRow
                icon={<LayoutGrid class="size-[17px]" strokeWidth={1.7} />}
                label="Connectors"
                onClick={() => setView('connectors')}
                right={<ChevronRight class="size-3.5" />}
              />
            </div>
          </Match>

          <Match when={view() === 'skills'}>
            <SettingsSubView title="Skills" onBack={() => setView('main')}>
              <RemoteSettingsList
                emptyLabel="No skills are available yet."
                endpoint="/api/v1/skills"
                itemKey="skills"
                manageHref="/agents"
                manageLabel="Manage skills"
              />
            </SettingsSubView>
          </Match>

          <Match when={view() === 'projects'}>
            <SettingsSubView title="Add to project" onBack={() => setView('main')}>
              <RemoteSettingsList
                emptyLabel="No projects are available yet."
                endpoint="/api/v1/projects"
                itemKey="projects"
              />
            </SettingsSubView>
          </Match>

          <Match when={view() === 'connectors'}>
            <SettingsSubView title="Connectors" onBack={() => setView('main')}>
              <RemoteSettingsList
                emptyLabel="No connectors are connected yet."
                endpoint="/api/v1/integrations/providers"
                itemKey="providers"
                manageHref="/settings/integrations"
                manageLabel="Connect more"
              />
            </SettingsSubView>
          </Match>
        </Switch>
      </div>
    </div>
  )
}

function ComposerMenuRow(props: {
  icon: JSX.Element
  label: string
  onClick: () => void
  right?: JSX.Element
}) {
  return (
    <button type="button" onClick={() => props.onClick()} class="velion-menu-row dashboard-composer-settings-row">
      {props.icon}
      <span class="velion-menu-label">{props.label}</span>
      {props.right}
    </button>
  )
}

function SettingsSubView(props: {
  children: JSX.Element
  onBack: () => void
  title: string
}) {
  return (
    <div class="velion-settings-view">
      <button type="button" onClick={() => props.onBack()} class="velion-menu-row dashboard-composer-settings-back">
        <ArrowLeft class="size-3.5" strokeWidth={1.8} />
        <span>{props.title}</span>
      </button>
      <div class="dashboard-composer-menu-divider" />
      {props.children}
    </div>
  )
}

function RemoteSettingsList(props: {
  emptyLabel: string
  endpoint: string
  itemKey: string
  manageHref?: string
  manageLabel?: string
}) {
  const [items] = createResource(
    () => [props.endpoint, props.itemKey] as const,
    ([endpoint, itemKey]) => loadComposerSettingsItems(endpoint, itemKey),
  )

  return (
    <>
      <Show
        when={!items.loading}
        fallback={
          <div class="dashboard-composer-settings-loading">
            <Loader2 class="size-3.5" />
            <span>Loading…</span>
          </div>
        }
      >
        <Show
          when={!items.error}
          fallback={
            <div class="dashboard-composer-settings-error">
              <p>Could not load from the gateway.</p>
              <small>{items.error instanceof Error ? items.error.message : 'Service unavailable'}</small>
            </div>
          }
        >
          <Show
            when={(items() ?? []).length > 0}
            fallback={<div class="dashboard-composer-settings-empty">{props.emptyLabel}</div>}
          >
            <For each={items()}>
              {(item) => <RemoteSettingsItemRow item={item} />}
            </For>
          </Show>
        </Show>
      </Show>

      <Show when={props.manageHref && props.manageLabel ? { href: props.manageHref, label: props.manageLabel } : null}>
        {(manage) => (
          <>
            <div class="dashboard-composer-menu-divider" />
            <A href={manage().href} class="velion-menu-row dashboard-composer-settings-link">
              <span class="dashboard-composer-settings-item-icon">
                <Briefcase class="size-3.5" />
              </span>
              <span class="velion-menu-label">{manage().label}</span>
            </A>
          </>
        )}
      </Show>
    </>
  )
}

function RemoteSettingsItemRow(props: { item: ComposerSettingsItem }) {
  return (
    <div class="velion-menu-row dashboard-composer-settings-data-row">
      <span class="dashboard-composer-settings-item-icon">
        <Briefcase class="size-3.5" />
      </span>
      <span class="dashboard-composer-settings-item-copy">
        <span class="velion-menu-label">{props.item.name}</span>
        <Show when={props.item.description}>
          <span class="velion-menu-meta">{props.item.description}</span>
        </Show>
      </span>
      <Show when={typeof props.item.connected === 'boolean'}>
        <span class="velion-menu-meta dashboard-composer-settings-item-status">
          {props.item.connected ? 'Ready' : 'Open'}
        </span>
      </Show>
    </div>
  )
}

function SplitText(props: { text: string }) {
  return (
    <span>
      <For each={props.text.split('')}>
        {(char, index) => (
          <span class="velion-split-char" style={{ 'animation-delay': `${index() * 25}ms` }}>
            {char === ' ' ? '\u00a0' : char}
          </span>
        )}
      </For>
    </span>
  )
}

type ComposerIconButtonVariant = 'chip' | 'toolbar'

function ComposerIconButton(props: {
  active?: boolean
  children: JSX.Element
  label: string
  onClick: () => void
  variant: ComposerIconButtonVariant
}) {
  const baseClass = () => props.variant === 'toolbar' ? 'dashboard-composer-toolbar-icon' : 'dashboard-composer-icon-chip'

  return (
    <button
      type="button"
      aria-label={props.label}
      aria-pressed={props.active}
      title={props.label}
      onClick={() => props.onClick()}
      class={cn(baseClass(), props.active ? `${baseClass()}--active` : '')}
    >
      {props.children}
    </button>
  )
}

function formatComposerTurnTime(date: Date) {
  return date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

function threadToHistoryItem(thread: ChatThreadSession): HistoryPanelItem {
  return {
    fallbackTime: thread.updatedAt,
    id: thread.threadId,
    meta: thread.preview || 'Chat thread',
    threadId: thread.threadId,
    title: thread.title || 'Untitled',
    updatedAt: thread.updatedAt,
  }
}

function turnToHistoryItem(turn: ComposerTurn): HistoryPanelItem {
  return {
    fallbackTime: turn.createdAt,
    id: turn.id,
    meta: turn.model,
    title: turn.body || 'Untitled',
    updatedAt: turn.createdAtIso,
  }
}

function historyGroups(items: HistoryPanelItem[]) {
  const now = new Date()
  const today = now.toDateString()
  const yesterday = new Date(now.getTime() - 86_400_000).toDateString()
  const groups = items.reduce<{
    today: HistoryPanelItem[]
    yesterday: HistoryPanelItem[]
    earlier: HistoryPanelItem[]
  }>(
    (accumulator, item) => {
      const date = new Date(item.updatedAt).toDateString()
      if (date === today) return { ...accumulator, today: [...accumulator.today, item] }
      if (date === yesterday) return { ...accumulator, yesterday: [...accumulator.yesterday, item] }
      return { ...accumulator, earlier: [...accumulator.earlier, item] }
    },
    { today: [], yesterday: [], earlier: [] },
  )

  return [
    { label: 'Today', items: groups.today, isToday: true },
    { label: 'Yesterday', items: groups.yesterday, isToday: false },
    { label: 'Earlier', items: groups.earlier, isToday: false },
  ] as const
}

function formatHistoryItemTime(item: HistoryPanelItem, isToday: boolean) {
  const date = new Date(item.updatedAt)
  if (Number.isNaN(date.getTime())) return item.fallbackTime

  return isToday
    ? date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
    : date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function panelPositionStyle(position: PanelPosition): JSX.CSSProperties {
  const style: JSX.CSSProperties = {
    'max-height': `${position.maxHeight}px`,
    position: 'fixed',
    'z-index': 'var(--velion-z-popover)',
  }
  if (position.bottom !== undefined) style.bottom = `${position.bottom}px`
  if (position.left !== undefined) style.left = `${position.left}px`
  if (position.right !== undefined) style.right = `${position.right}px`
  if (position.top !== undefined) style.top = `${position.top}px`
  return style
}
