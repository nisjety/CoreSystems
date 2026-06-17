import { A } from '@solidjs/router'
import {
  BringToFront,
  Copy,
  Eye,
  GripHorizontal,
  Image,
  Link,
  Monitor,
  MousePointer2,
  PanelRight,
  PenLine,
  Plus,
  Redo2,
  Send,
  SendToBack,
  Smartphone,
  Sparkles,
  Trash2,
  Type,
  Undo2,
  Video,
} from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Match, onCleanup, onMount, Show, Switch } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  buildSocialDraftBody,
  CANVAS_HEIGHT,
  CANVAS_PADDING,
  CANVAS_WIDTH,
  clamp,
  clampToCanvas,
  cloneBlocks,
  constrainBlock,
  createBlock,
  DUPLICATE_OFFSET,
  GRID_SIZE,
  initialBlocks,
  isEditableTarget,
  loadStudioWorkspace,
  MIN_BLOCK_HEIGHT,
  MIN_BLOCK_WIDTH,
  normalizeStudioBlock,
  serializeBlocks,
  snapToGrid,
  type StudioBlock,
  type StudioBlockKind,
  type StudioLayoutField,
  type StudioPersistenceAction,
  type StudioPersistenceSource,
  type StudioPreviewDevice,
  type StudioProject,
} from '@/features/studio/lib/studio-canvas-model'
import {
  createStudioProject,
  exportStudioProjectToSocialDraft,
  saveStudioProject,
} from '@/shared/api/studio-client'
import { cn } from '@/shared/lib/cn'

type StudioSection = 'canvas' | 'campaigns' | 'templates'

type DragState = {
  blockId: string
  offsetX: number
  offsetY: number
  startBlocks: StudioBlock[]
}

const addBlockOptions: Array<{
  kind: StudioBlockKind
  label: string
  icon: typeof Type
}> = [
  { kind: 'image', label: 'Image', icon: Image },
  { kind: 'video', label: 'Video', icon: Video },
  { kind: 'link', label: 'Link', icon: Link },
  { kind: 'text', label: 'Text', icon: Type },
  { kind: 'social', label: 'Post', icon: PenLine },
]

export default function StudioPage(props: { section?: StudioSection }) {
  const section = () => props.section ?? 'canvas'

  return (
    <Switch>
      <Match when={section() === 'canvas'}>
        <StudioCanvasPage />
      </Match>
      <Match when={section() === 'campaigns'}>
        <StudioLibraryPage
          eyebrow="Studio campaign planner"
          title="Campaign planner"
          description="Shape a launch, social pack, email pack, and approval path before it moves into Social or Agents."
          active="campaigns"
        />
      </Match>
      <Match when={section() === 'templates'}>
        <StudioLibraryPage
          eyebrow="Reusable canvas systems"
          title="Templates"
          description="Start from approved layouts for launches, founder updates, UGC scripts, case studies, and weekly content systems."
          active="templates"
        />
      </Match>
    </Switch>
  )
}

function StudioCanvasPage() {
  let canvasRef: HTMLDivElement | undefined
  let loadedProjectId: string | null = null
  const [blocks, setBlocks] = createSignal<StudioBlock[]>(initialBlocks)
  const [selectedBlockId, setSelectedBlockId] = createSignal(initialBlocks[0]?.id ?? '')
  const [previewDevice, setPreviewDevice] = createSignal<StudioPreviewDevice>('desktop')
  const [projectId, setProjectId] = createSignal<string | null>(null)
  const [orgId, setOrgId] = createSignal('')
  const [projectTitle, setProjectTitle] = createSignal('Launch canvas')
  const [persistenceSource, setPersistenceSource] = createSignal<StudioPersistenceSource>('fallback')
  const [persistenceMessage, setPersistenceMessage] = createSignal('Local canvas')
  const [busyAction, setBusyAction] = createSignal<StudioPersistenceAction | null>(null)
  const [history, setHistory] = createSignal<StudioBlock[][]>([])
  const [future, setFuture] = createSignal<StudioBlock[][]>([])
  const [drag, setDrag] = createSignal<DragState | null>(null)
  const [workspace] = createResource(loadStudioWorkspace)
  const selectedBlock = createMemo(() => blocks().find((block) => block.id === selectedBlockId()))
  const canvasSummary = createMemo(() => `${blocks().length} blocks · ${persistenceMessage()}`)
  const canPersist = createMemo(() => Boolean(orgId()) && !busyAction())
  const selectedToolbarStyle = createMemo(() => {
    const block = selectedBlock()
    if (!block) return {}

    return {
      left: `${clamp(block.x + block.width / 2, 132, CANVAS_WIDTH - 132)}px`,
      top: `${Math.max(18, block.y - 58)}px`,
    }
  })

  createEffect(() => {
    const nextWorkspace = workspace()
    if (!nextWorkspace) {
      setPersistenceMessage('Loading project')
      return
    }

    setOrgId(nextWorkspace.orgId)
    setPersistenceSource(nextWorkspace.source)

    if (!nextWorkspace.project) {
      setPersistenceMessage(nextWorkspace.orgId ? 'Ready to save' : 'Local canvas')
      return
    }

    if (nextWorkspace.project.id === loadedProjectId) return
    loadedProjectId = nextWorkspace.project.id
    const nextBlocks = nextWorkspace.project.blocks.length
      ? nextWorkspace.project.blocks.map(normalizeStudioBlock)
      : cloneBlocks(initialBlocks)

    setProjectId(nextWorkspace.project.id)
    setProjectTitle(nextWorkspace.project.title || 'Launch canvas')
    setBlocks(nextBlocks)
    setSelectedBlockId(nextWorkspace.project.selectedBlockId ?? nextBlocks[0]?.id ?? '')
    setHistory([])
    setFuture([])
    setPersistenceMessage('Saved project')
  })

  const pushHistory = (snapshot: StudioBlock[]) => {
    setHistory((current) => [...current.slice(-19), cloneBlocks(snapshot)])
    setFuture([])
  }

  const addBlock = (kind: StudioBlockKind) => {
    const current = blocks()
    const count = current.filter((block) => block.kind === kind).length + 1
    pushHistory(current)
    const next = createBlock(kind, count)
    setBlocks([...current, next])
    setSelectedBlockId(next.id)
  }

  const updateSelectedText = (field: 'title' | 'body', value: string) => {
    const selected = selectedBlock()
    if (!selected) return
    setBlocks((current) => current.map((block) => (
      block.id === selected.id ? { ...block, [field]: value } : block
    )))
  }

  const updateSelectedLayout = (field: StudioLayoutField, value: string) => {
    const selected = selectedBlock()
    const nextValue = Number(value)
    if (!selected || !Number.isFinite(nextValue)) return

    setBlocks((current) => current.map((block) => (
      block.id === selected.id ? constrainBlock({ ...block, [field]: nextValue }) : block
    )))
  }

  const persistCurrentProject = async (): Promise<StudioProject | null> => {
    const activeOrgId = orgId()
    if (!activeOrgId) {
      setPersistenceSource('fallback')
      setPersistenceMessage('Local canvas')
      return null
    }

    const payload = {
      title: projectTitle(),
      blocks: serializeBlocks(blocks()),
      selectedBlockId: selectedBlockId() || null,
    }
    const currentProjectId = projectId()
    const result = currentProjectId
      ? await saveStudioProject(activeOrgId, currentProjectId, payload)
      : await createStudioProject(activeOrgId, payload)

    loadedProjectId = result.project.id
    setProjectId(result.project.id)
    setProjectTitle(result.project.title)
    setPersistenceSource('live')
    setPersistenceMessage('Saved project')
    return result.project
  }

  const saveCurrentProject = async () => {
    if (busyAction()) return

    setBusyAction('save')
    setPersistenceMessage(orgId() ? 'Saving project' : 'Local canvas')
    try {
      await persistCurrentProject()
    } catch (reason) {
      setPersistenceSource('fallback')
      setPersistenceMessage(reason instanceof Error ? reason.message : 'Save failed')
    } finally {
      setBusyAction(null)
    }
  }

  const exportCurrentProject = async () => {
    if (busyAction()) return

    setBusyAction('export')
    setPersistenceMessage(orgId() ? 'Exporting draft' : 'Local canvas')
    try {
      const project = await persistCurrentProject()
      if (!project || !orgId()) return

      const result = await exportStudioProjectToSocialDraft(orgId(), project.id, {
        title: projectTitle(),
        body: buildSocialDraftBody(blocks()),
        platforms: ['linkedin', 'x'],
      })
      loadedProjectId = result.project.id
      setProjectId(result.project.id)
      setPersistenceSource('live')
      setPersistenceMessage('Social draft created')
    } catch (reason) {
      setPersistenceSource('fallback')
      setPersistenceMessage(reason instanceof Error ? reason.message : 'Export failed')
    } finally {
      setBusyAction(null)
    }
  }

  const duplicateSelected = () => {
    const selected = selectedBlock()
    if (!selected) return

    const current = blocks()
    pushHistory(current)
    const copy = constrainBlock({
      ...selected,
      id: `${selected.kind}_${Date.now()}_copy`,
      title: `${selected.title} copy`,
      x: snapToGrid(selected.x + DUPLICATE_OFFSET),
      y: snapToGrid(selected.y + DUPLICATE_OFFSET),
    })

    setBlocks([...current, copy])
    setSelectedBlockId(copy.id)
  }

  const deleteSelected = () => {
    const selected = selectedBlock()
    if (!selected) return

    const current = blocks()
    const selectedIndex = current.findIndex((block) => block.id === selected.id)
    const next = current.filter((block) => block.id !== selected.id)
    pushHistory(current)
    setBlocks(next)
    setSelectedBlockId(next[Math.min(selectedIndex, next.length - 1)]?.id ?? '')
  }

  const moveSelectedLayer = (direction: 'front' | 'back') => {
    const selected = selectedBlock()
    if (!selected) return

    const current = blocks()
    const rest = current.filter((block) => block.id !== selected.id)
    pushHistory(current)
    setBlocks(direction === 'front' ? [...rest, selected] : [selected, ...rest])
    setSelectedBlockId(selected.id)
  }

  const nudgeSelected = (deltaX: number, deltaY: number) => {
    const selected = selectedBlock()
    if (!selected) return

    const current = blocks()
    pushHistory(current)
    setBlocks((snapshot) => snapshot.map((block) => (
      block.id === selected.id
        ? constrainBlock({ ...block, x: block.x + deltaX, y: block.y + deltaY })
        : block
    )))
  }

  const undo = () => {
    const snapshots = history()
    const previous = snapshots.at(-1)
    if (!previous) return
    setFuture((current) => [cloneBlocks(blocks()), ...current])
    setHistory(snapshots.slice(0, -1))
    setBlocks(cloneBlocks(previous))
    setSelectedBlockId(previous[0]?.id ?? '')
  }

  const redo = () => {
    const [next, ...rest] = future()
    if (!next) return
    setHistory((current) => [...current, cloneBlocks(blocks())])
    setFuture(rest)
    setBlocks(cloneBlocks(next))
    setSelectedBlockId(next[0]?.id ?? '')
  }

  const startDrag = (event: PointerEvent & { currentTarget: HTMLElement }, block: StudioBlock) => {
    if (event.button > 0) return
    const canvas = canvasRef?.getBoundingClientRect()
    if (!canvas) return

    event.preventDefault()
    event.currentTarget.setPointerCapture?.(event.pointerId)
    setSelectedBlockId(block.id)
    setDrag({
      blockId: block.id,
      offsetX: event.clientX - canvas.left - block.x,
      offsetY: event.clientY - canvas.top - block.y,
      startBlocks: blocks(),
    })
  }

  const moveDrag = (event: PointerEvent) => {
    const state = drag()
    const canvas = canvasRef?.getBoundingClientRect()
    if (!state || !canvas) return
    setBlocks((current) => current.map((block) => {
      if (block.id !== state.blockId) return block
      const rawX = event.clientX - canvas.left - state.offsetX
      const rawY = event.clientY - canvas.top - state.offsetY
      const nextX = event.altKey ? rawX : snapToGrid(rawX)
      const nextY = event.altKey ? rawY : snapToGrid(rawY)

      return {
        ...block,
        x: clampToCanvas(nextX, block.width, CANVAS_WIDTH),
        y: clampToCanvas(nextY, block.height, CANVAS_HEIGHT),
      }
    }))
  }

  const stopDrag = () => {
    const state = drag()
    if (!state) return
    const changed = JSON.stringify(state.startBlocks) !== JSON.stringify(blocks())
    if (changed) pushHistory(state.startBlocks)
    setDrag(null)
  }

  const clearSelectionFromCanvas = (event: PointerEvent & { currentTarget: HTMLDivElement }) => {
    if (event.target === event.currentTarget) {
      setSelectedBlockId('')
    }
  }

  const handleCanvasKeyDown = (event: KeyboardEvent) => {
    if (isEditableTarget(event.target)) return

    if (event.key === 'Escape') {
      setSelectedBlockId('')
      return
    }

    if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault()
      deleteSelected()
      return
    }

    const distance = event.shiftKey ? GRID_SIZE * 5 : GRID_SIZE
    switch (event.key) {
      case 'ArrowUp':
        event.preventDefault()
        nudgeSelected(0, -distance)
        break
      case 'ArrowRight':
        event.preventDefault()
        nudgeSelected(distance, 0)
        break
      case 'ArrowDown':
        event.preventDefault()
        nudgeSelected(0, distance)
        break
      case 'ArrowLeft':
        event.preventDefault()
        nudgeSelected(-distance, 0)
        break
    }
  }

  onMount(() => {
    window.addEventListener('pointermove', moveDrag)
    window.addEventListener('pointerup', stopDrag)
    document.addEventListener('keydown', handleCanvasKeyDown)
    onCleanup(() => {
      window.removeEventListener('pointermove', moveDrag)
      window.removeEventListener('pointerup', stopDrag)
      document.removeEventListener('keydown', handleCanvasKeyDown)
    })
  })

  return (
    <div class="velion-studio-page">
      <div class="velion-studio-board-shell">
        <header class="velion-studio-topbar">
          <div class="velion-studio-profile">
            <img src="https://images.unsplash.com/photo-1494790108377-be9c29b29330?auto=format&fit=crop&w=160&q=80" alt="" />
            <div>
              <strong>Launch canvas</strong>
              <span>{canvasSummary()}</span>
            </div>
          </div>
          <div class="velion-studio-topbar__actions">
            <button
              type="button"
              class="velion-studio-pill-button"
              onClick={exportCurrentProject}
              disabled={!canPersist()}
            >
              <Send size={15} />
              {busyAction() === 'export' ? 'Exporting' : 'Send to drafts'}
            </button>
            <button
              type="button"
              class={cn('velion-studio-publish-button', persistenceSource() === 'live' && 'is-live')}
              onClick={saveCurrentProject}
              disabled={!canPersist()}
            >
              <span />
              {busyAction() === 'save' ? 'Saving' : 'Save'}
            </button>
          </div>
        </header>

        <main class="velion-studio-canvas-wrap" aria-label="Studio canvas workspace">
          <div
            ref={canvasRef}
            class={cn(
              'velion-studio-canvas',
              drag() && 'is-drop-active',
              !blocks().length && 'is-empty',
              previewDevice() === 'mobile' && 'is-mobile-preview',
            )}
            onPointerDown={clearSelectionFromCanvas}
          >
            <For each={blocks()}>
              {(block) => (
                <StudioCanvasBlock
                  block={block}
                  dragging={drag()?.blockId === block.id}
                  selected={selectedBlockId() === block.id}
                  onSelect={() => setSelectedBlockId(block.id)}
                  onPointerDown={(event) => startDrag(event, block)}
                />
              )}
            </For>
            <Show when={selectedBlock()}>
              {(block) => (
                <div
                  class="velion-studio-block-toolbar"
                  style={selectedToolbarStyle()}
                  aria-label={`Selected block toolbar for ${block().title}`}
                  onPointerDown={(event) => event.stopPropagation()}
                >
                  <span class="velion-studio-block-toolbar__handle" aria-hidden="true">
                    <GripHorizontal size={16} />
                  </span>
                  <button type="button" onClick={duplicateSelected} aria-label="Duplicate selected block">
                    <Copy size={15} />
                  </button>
                  <button type="button" onClick={() => moveSelectedLayer('back')} aria-label="Send selected block to back">
                    <SendToBack size={15} />
                  </button>
                  <button type="button" onClick={() => moveSelectedLayer('front')} aria-label="Bring selected block to front">
                    <BringToFront size={15} />
                  </button>
                  <button type="button" class="is-danger" onClick={deleteSelected} aria-label="Delete selected block">
                    <Trash2 size={15} />
                  </button>
                </div>
              )}
            </Show>
            <Show when={!blocks().length}>
              <div class="velion-studio-empty-state">
                <span>
                  <Plus size={20} />
                </span>
                <strong>Start a Studio board</strong>
                <p>Add a campaign note, image, or social draft to build the canvas.</p>
                <div>
                  <button type="button" onClick={() => addBlock('text')}>Text</button>
                  <button type="button" onClick={() => addBlock('image')}>Image</button>
                  <button type="button" onClick={() => addBlock('social')}>Post</button>
                </div>
              </div>
            </Show>
          </div>
        </main>

        <div class="velion-studio-undo" aria-label="Canvas history controls">
          <button type="button" onClick={undo} disabled={!history().length} aria-label="Undo">
            <Undo2 size={17} />
          </button>
          <button type="button" onClick={redo} disabled={!future().length} aria-label="Redo">
            <Redo2 size={17} />
          </button>
        </div>

        <div class="velion-studio-dock" aria-label="Add canvas blocks">
          <button type="button" class="is-active" aria-label="Select blocks">
            <MousePointer2 size={18} />
          </button>
          <For each={addBlockOptions}>
            {(option) => (
              <button type="button" onClick={() => addBlock(option.kind)} aria-label={`Add ${option.label} block`}>
                <Dynamic component={option.icon} size={18} />
              </button>
            )}
          </For>
        </div>

        <div class="velion-studio-device-switch" aria-label="Preview device">
          <button
            type="button"
            class={cn(previewDevice() === 'desktop' && 'is-active')}
            onClick={() => setPreviewDevice('desktop')}
            aria-label="Desktop preview"
            aria-pressed={previewDevice() === 'desktop'}
          >
            <Monitor size={17} />
          </button>
          <button
            type="button"
            class={cn(previewDevice() === 'mobile' && 'is-active')}
            onClick={() => setPreviewDevice('mobile')}
            aria-label="Mobile preview"
            aria-pressed={previewDevice() === 'mobile'}
          >
            <Smartphone size={17} />
          </button>
        </div>

        <div class="velion-studio-preview-actions">
          <button type="button">
            <Eye size={16} />
            Preview
          </button>
          <button type="button" aria-label="Open inspector">
            <PanelRight size={17} />
          </button>
        </div>

        <aside class="velion-studio-inspector">
          <Show when={selectedBlock()} fallback={<p>Select a block</p>}>
            {(block) => (
              <>
                <span class="velion-studio-inspector__eyebrow">{block().kind}</span>
                <label>
                  Title
                  <input value={block().title} onInput={(event) => updateSelectedText('title', event.currentTarget.value)} />
                </label>
                <label>
                  Notes
                  <textarea value={block().body ?? ''} onInput={(event) => updateSelectedText('body', event.currentTarget.value)} />
                </label>
                <div class="velion-studio-inspector__layout">
                  <label>
                    X
                    <input type="number" min={CANVAS_PADDING} value={Math.round(block().x)} onInput={(event) => updateSelectedLayout('x', event.currentTarget.value)} />
                  </label>
                  <label>
                    Y
                    <input type="number" min={CANVAS_PADDING} value={Math.round(block().y)} onInput={(event) => updateSelectedLayout('y', event.currentTarget.value)} />
                  </label>
                  <label>
                    W
                    <input type="number" min={MIN_BLOCK_WIDTH} value={Math.round(block().width)} onInput={(event) => updateSelectedLayout('width', event.currentTarget.value)} />
                  </label>
                  <label>
                    H
                    <input type="number" min={MIN_BLOCK_HEIGHT} value={Math.round(block().height)} onInput={(event) => updateSelectedLayout('height', event.currentTarget.value)} />
                  </label>
                </div>
                <div class="velion-studio-inspector__links">
                  <A href="/social/calendar">Open calendar</A>
                  <A href="/knowledge">Attach assets</A>
                </div>
              </>
            )}
          </Show>
        </aside>
      </div>
    </div>
  )
}

function StudioCanvasBlock(props: {
  block: StudioBlock
  dragging: boolean
  onPointerDown: (event: PointerEvent & { currentTarget: HTMLElement }) => void
  onSelect: () => void
  selected: boolean
}) {
  return (
    <article
      class={cn(
        'velion-studio-block',
        `velion-studio-block--${props.block.kind}`,
        props.selected && 'is-selected',
        props.dragging && 'is-dragging',
      )}
      style={{
        left: `${props.block.x}px`,
        top: `${props.block.y}px`,
        width: `${props.block.width}px`,
        height: `${props.block.height}px`,
      }}
      aria-label={`${props.block.kind} block: ${props.block.title}`}
      onClick={() => props.onSelect()}
      onPointerDown={(event) => props.onPointerDown(event)}
      onFocus={() => props.onSelect()}
      tabIndex={0}
    >
      <div class="velion-studio-block__surface">
        <Switch>
          <Match when={props.block.kind === 'image' || props.block.kind === 'profile'}>
            <img src={props.block.imageUrl ?? undefined} alt="" />
            <div class="velion-studio-block__caption">
              <strong>{props.block.title}</strong>
              <span>{props.block.body}</span>
            </div>
          </Match>
          <Match when={props.block.kind === 'brand'}>
            <div class="velion-studio-brandmark">{props.block.title}</div>
            <p>{props.block.body}</p>
          </Match>
          <Match when={props.block.kind === 'link'}>
            <div class="velion-studio-link-preview">
              <span>X</span>
            </div>
            <strong>{props.block.title}</strong>
            <p>{props.block.body}</p>
          </Match>
          <Match when={props.block.kind === 'video'}>
            <div class="velion-studio-video-preview">
              <Video size={34} />
            </div>
            <strong>{props.block.title}</strong>
            <p>{props.block.body}</p>
          </Match>
          <Match when={props.block.kind === 'social'}>
            <span class="velion-studio-social-label">Social post</span>
            <strong>{props.block.title}</strong>
            <p>{props.block.body}</p>
          </Match>
          <Match when={props.block.kind === 'text'}>
            <strong>{props.block.title}</strong>
            <p>{props.block.body}</p>
          </Match>
        </Switch>
      </div>
      <Show when={props.selected}>
        <span class="velion-studio-selection-handle is-nw" aria-hidden="true" />
        <span class="velion-studio-selection-handle is-ne" aria-hidden="true" />
        <span class="velion-studio-selection-handle is-sw" aria-hidden="true" />
        <span class="velion-studio-selection-handle is-se" aria-hidden="true" />
        <span class="velion-studio-block__size-pill">
          {Math.round(props.block.width)} x {Math.round(props.block.height)}
        </span>
      </Show>
    </article>
  )
}

function StudioLibraryPage(props: {
  active: 'campaigns' | 'templates'
  description: string
  eyebrow: string
  title: string
}) {
  const cards = createMemo(() => props.active === 'campaigns'
    ? [
        ['Product launch', 'Map social, email, landing, and approval work.'],
        ['Weekly trust loop', 'Turn support patterns into public content.'],
        ['Founder narrative', 'Shape a multi-channel executive story.'],
      ]
    : [
        ['Social launch board', 'Canvas blocks for calendar-ready post packs.'],
        ['UGC script pack', 'Hooks, scenes, captions, and asset placeholders.'],
        ['Trend remix board', 'Reference, angle, draft, and approval blocks.'],
      ])

  return (
    <div class="velion-studio-library-page">
      <section class="velion-studio-library-hero">
        <span>{props.eyebrow}</span>
        <h1>{props.title}</h1>
        <p>{props.description}</p>
        <A href="/studio/canvas">Open canvas</A>
      </section>
      <div class="velion-studio-library-grid">
        <For each={cards()}>
          {(card) => (
            <article>
              <Sparkles size={18} />
              <h2>{card[0]}</h2>
              <p>{card[1]}</p>
            </article>
          )}
        </For>
      </div>
    </div>
  )
}
