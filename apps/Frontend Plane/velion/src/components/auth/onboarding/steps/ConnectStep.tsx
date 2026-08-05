'use client'

/**
 * Step 4 — knowledge connectors with live graph reveal.
 *
 * Left pane: Chatbase-style grouped source picker. Sources are
 * organised into three categories (Chat, Docs, Tools) so the column
 * reads like Chatbase's "Sources" rail instead of a flat checklist.
 * Clicking a row creates a Nango connect session through integration-core,
 * embeds Nango Connect inside the onboarding card, records the pick only
 * after Nango reports success, and nudges the graph poller.
 *
 * Right pane: SVG node-link graph driven by real entities the user's
 * org has in Data Plane v2 (`graph-index-rs`). The component
 * polls the preview route every 5 s while this step is on screen and
 * computes a stable layout per node id so re-fetches don't bounce
 * everything around the canvas. If Data Plane has not indexed the
 * new connection yet the route returns real connection metadata from
 * integration-core, never fabricated graph facts.
 *
 * The graph layout is deterministic-radial: every node id hashes to
 * its own angle + radius around the org anchor. That's enough for
 * the "feel like Obsidian" effect without dragging in a force-
 * directed library (the verevon bundle is already heavy).
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { X } from 'lucide-react'

import { useAuth } from '@/components/auth/hooks/use-auth'

import {
  formatOnboardingText,
  type OnboardingLocale,
  useOnboardingCopy,
} from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type { OrganizationPayload, WebsitePayload } from '../state/types'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

interface ConnectorMeta {
  id: string
  label: string
  hint: string
  category: 'chat' | 'docs' | 'tools'
  provider: string
  sources: string[]
}

interface ConnectCopy {
  categories: Record<ConnectorMeta['category'], string>
  continue: string
  skip: string
  signedOut: string
  noOrg: string
  couldNotConnect: string
  connectFlowFailed: string
  counts: string
  graphCounts: string
  status: {
    opening: string
    connected: string
    add: string
  }
  overlay: {
    eyebrow: string
    close: string
    title: string
  }
}

type ConnectOverlayCopy = ConnectCopy['overlay']

const CONNECTORS: ConnectorMeta[] = [
  {
    id: 'slack',
    label: 'Slack',
    hint: 'Channels + threads',
    category: 'chat',
    provider: 'slack',
    sources: ['messages'],
  },
  {
    id: 'microsoft365',
    label: 'Microsoft 365',
    hint: 'Teams, Outlook, SharePoint, OneDrive',
    category: 'chat',
    provider: 'microsoft',
    sources: ['teams', 'outlook', 'sharepoint', 'onedrive'],
  },
  {
    id: 'notion',
    label: 'Notion',
    hint: 'Pages + databases',
    category: 'docs',
    provider: 'notion',
    sources: ['pages', 'databases'],
  },
  {
    id: 'gdrive',
    label: 'Google Drive',
    hint: 'Docs + Sheets',
    category: 'docs',
    provider: 'google-drive',
    sources: ['google_drive', 'documents'],
  },
  {
    id: 'github',
    label: 'GitHub',
    hint: 'README + issues',
    category: 'tools',
    provider: 'github',
    sources: ['issues'],
  },
]

const CONNECTOR_HINTS: Record<OnboardingLocale, Record<string, string>> = {
  nb: {
    slack: 'Kanaler + tråder',
    microsoft365: 'Teams, Outlook, SharePoint, OneDrive',
    notion: 'Sider + databaser',
    gdrive: 'Dokumenter + regneark',
    github: 'README + issues',
  },
  en: {
    slack: 'Channels + threads',
    microsoft365: 'Teams, Outlook, SharePoint, OneDrive',
    notion: 'Pages + databases',
    gdrive: 'Docs + Sheets',
    github: 'README + issues',
  },
}

interface PreviewNode {
  id: string
  label: string
  group: string
}

interface PreviewEdge {
  a: string
  b: string
}

interface PreviewResponse {
  nodes: PreviewNode[]
  edges: PreviewEdge[]
  counts: { nodes: number; edges: number; groups: number }
  warning?: string
}

interface ConnectFrameState {
  connector: ConnectorMeta
  sessionId?: string
  url: string
}

interface NangoConnectMessage {
  type: 'ready' | 'connect' | 'error' | 'close'
  payload?: unknown
}

const GRAPH_POLL_INTERVAL_MS = 5_000
const GRAPH_PREVIEW_RETRY_DELAYS_MS = [450, 1_200]
const EMPTY_PREVIEW_NODES: PreviewNode[] = []
const MICROSOFT_CONNECTOR_ALIASES = new Set([
  'teams',
  'sharepoint',
  'onedrive',
  'outlook',
  'm365',
  'microsoft365',
  'microsoft-365',
])

export function ConnectStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy } = useOnboardingCopy()
  const auth = useAuth()
  const initial = machine.state.connectors
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(initial.map((c) => normalizeConnectorId(c.id))),
  )
  const [graph, setGraph] = useState<PreviewResponse | null>(null)
  const [warning, setWarning] = useState<string | null>(null)
  const [connectError, setConnectError] = useState<string | null>(null)
  const [connectingId, setConnectingId] = useState<string | null>(null)
  const [connectFrame, setConnectFrame] = useState<ConnectFrameState | null>(null)
  const iframeRef = useRef<HTMLIFrameElement | null>(null)
  const pollTickRef = useRef(0)
  const graphRequestInFlightRef = useRef(false)
  const queuedGraphRefreshRef = useRef(false)

  const refreshGraph = useCallback(async () => {
    if (graphRequestInFlightRef.current) {
      queuedGraphRefreshRef.current = true
      return
    }
    graphRequestInFlightRef.current = true
    try {
      const payload = await loadGraphPreview()
      if (!payload) return
      setGraph(payload)
      if (payload.warning) setWarning(payload.warning)
      else setWarning(null)
    } catch {
      // Swallow — keep showing whatever we last had.
    } finally {
      graphRequestInFlightRef.current = false
      if (queuedGraphRefreshRef.current) {
        queuedGraphRefreshRef.current = false
        void refreshGraph()
      }
    }
  }, [])

  const completeConnector = useCallback(
    (connector: ConnectorMeta) => {
      machine.addConnector({
        id: connector.id,
        label: connector.label,
        authedAt: new Date().toISOString(),
      })
      setPicked((prev) => {
        if (prev.has(connector.id)) return prev
        return new Set([...prev, connector.id])
      })
      setConnectFrame(null)
      setConnectingId(null)
      setConnectError(null)
      pollTickRef.current += 1
      void refreshGraph().catch(() => undefined)
    },
    [machine, refreshGraph],
  )

  const toggle = useCallback(
    async (connector: ConnectorMeta) => {
      if (picked.has(connector.id)) {
        connectorStateIds(connector.id).forEach((id) => machine.removeConnector(id))
        setPicked((prev) => {
          const next = new Set(prev)
          next.delete(connector.id)
          return next
        })
        setConnectError(null)
      } else {
        if (!auth.user?.id || !auth.user.email) {
          setConnectError(copy.connect.signedOut)
          return
        }
        setConnectingId(connector.id)
        setConnectError(null)
        try {
          let orgId = machine.state.organization?.id
          if (!orgId) {
            const integrationsResponse = await fetch('/api/knowledge/integrations', {
              method: 'GET',
              credentials: 'include',
              cache: 'no-store',
            })
            const integrations = (await integrationsResponse.json()) as {
              orgId?: string
              error?: string
            }
            if (!integrationsResponse.ok || !integrations.orgId) {
              throw new Error(integrations.error || copy.connect.noOrg)
            }
            orgId = integrations.orgId
          }

          const response = await fetch('/api/connections/create', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            credentials: 'include',
            body: JSON.stringify({
              org_id: orgId,
              user_id: auth.user.id,
              user_email: auth.user.email,
              provider: connector.provider,
              sources: connector.sources,
            }),
          })
          const payload = (await response.json()) as {
            authorization_url?: string
            connect_link?: string
            session_id?: string
            error?: string
          }
          if (!response.ok) {
            throw new Error(
              payload.error ||
                formatOnboardingText(copy.connect.couldNotConnect, {
                  label: connector.label,
                }),
            )
          }

          const target = payload.connect_link || payload.authorization_url
          if (!target) {
            throw new Error(
              formatOnboardingText(copy.connect.couldNotConnect, {
                label: connector.label,
              }),
            )
          }

          setConnectFrame({
            connector,
            sessionId: payload.session_id,
            url: toEmbeddedConnectUrl(target),
          })
        } catch (error) {
          setConnectError(
            error instanceof Error
              ? error.message
              : formatOnboardingText(copy.connect.couldNotConnect, {
                  label: connector.label,
                }),
          )
          setConnectingId(null)
          return
        }
      }
      // Nudge the poller — clicking a source should refresh the graph
      // even before the next 5 s tick.
      pollTickRef.current += 1
      void refreshGraph().catch(() => undefined)
    },
    [auth.user?.email, auth.user?.id, copy.connect, machine, picked, refreshGraph],
  )

  useEffect(() => {
    if (!connectFrame) return

    const handleMessage = (event: MessageEvent) => {
      if (event.source !== iframeRef.current?.contentWindow) return
      const message = parseNangoConnectMessage(event.data)
      if (!message) return

      if (message.type === 'connect') {
        completeConnector(connectFrame.connector)
        return
      }

      if (message.type === 'error') {
        setConnectError(
          connectMessageError(message.payload, copy.connect.connectFlowFailed),
        )
        setConnectFrame(null)
        setConnectingId(null)
        return
      }

      if (message.type === 'close') {
        setConnectFrame(null)
        setConnectingId(null)
      }
    }

    window.addEventListener('message', handleMessage)
    return () => window.removeEventListener('message', handleMessage)
  }, [completeConnector, connectFrame, copy.connect.connectFlowFailed])

  // First fetch + poll loop. Poll only while this step is mounted so
  // we don't leak intervals into other steps.
  useEffect(() => {
    void refreshGraph()
    const id = window.setInterval(() => {
      void refreshGraph()
    }, GRAPH_POLL_INTERVAL_MS)
    return () => window.clearInterval(id)
  }, [refreshGraph])

  const submit = () => {
    machine.goTo('social-proof')
  }

  const sourceCount = picked.size + (machine.state.website?.url ? 1 : 0)
  const previewGraph = useMemo(
    () =>
      withWebsiteSource(
        graph,
        machine.state.website,
        machine.state.organization,
      ),
    [graph, machine.state.organization, machine.state.website],
  )
  const counts = previewGraph?.counts ?? { nodes: 0, edges: 0, groups: 0 }

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.connect.eyebrow}</StepEyebrow>
        <StepTitle>{copy.connect.title}</StepTitle>
        <StepDescription>{copy.connect.description}</StepDescription>

        <div className="flex flex-col gap-5">
          {(['chat', 'docs', 'tools'] as const).map((cat) => (
            <SourceCategory
              key={cat}
              title={copy.connect.categories[cat]}
              items={CONNECTORS.filter((c) => c.category === cat)}
              picked={picked}
              connectingId={connectingId}
              onToggle={toggle}
              locale={locale}
              copy={copy.connect}
            />
          ))}
        </div>

        {connectError && (
          <p className="font-inter text-[12px] leading-5 text-[#B42318]">
            {connectError}
          </p>
        )}

        <div className="flex items-center justify-between pt-2">
          <div className="flex items-center gap-4">
            <PrimaryButton onClick={submit}>{copy.connect.continue}</PrimaryButton>
            <SkipLink onClick={submit}>{copy.connect.skip}</SkipLink>
          </div>
          <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-[#A09890]">
            {formatOnboardingText(copy.connect.counts, {
              sources: sourceCount,
              nodes: counts.nodes,
              edges: counts.edges,
            })}
          </p>
        </div>
      </LeftPane>

      <RightPane showIcons={false}>
        <GraphReveal
          graph={previewGraph}
          warning={warning}
          sourceCount={sourceCount}
          countsTemplate={copy.connect.graphCounts}
        />
      </RightPane>

      {connectFrame && (
        <IntegrationConnectOverlay
          frame={connectFrame}
          iframeRef={iframeRef}
          copy={copy.connect.overlay}
          onClose={() => {
            setConnectFrame(null)
            setConnectingId(null)
          }}
        />
      )}
    </>
  )
}

function SourceCategory({
  title,
  items,
  picked,
  connectingId,
  onToggle,
  locale,
  copy,
}: {
  title: string
  items: ConnectorMeta[]
  picked: Set<string>
  connectingId: string | null
  onToggle: (connector: ConnectorMeta) => void
  locale: OnboardingLocale
  copy: ConnectCopy
}) {
  return (
    <div>
      <p className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
        {title}
      </p>
      <ul className="mt-2 flex flex-col gap-1.5">
        {items.map((c) => {
          const active = picked.has(c.id)
          const connecting = connectingId === c.id
          return (
            <li key={c.id}>
              <button
                type="button"
                onClick={() => onToggle(c)}
                aria-pressed={active}
                disabled={connectingId !== null}
                className={`flex w-full items-center justify-between rounded-md border px-3.5 py-2.5 text-left transition-colors ${
                  active
                    ? 'border-[#1F1B17] bg-[#1F1B17] text-white'
                    : 'border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890] disabled:cursor-wait disabled:opacity-60'
                }`}
              >
                <span className="flex flex-col">
                  <span className="font-inter text-[12.5px]">
                    {c.label}
                  </span>
                  <span
                    className={`mt-0.5 font-inter text-[11px] ${
                      active ? 'text-white/70' : 'text-[#6B6660]'
                    }`}
                  >
                    {CONNECTOR_HINTS[locale][c.id] ?? c.hint}
                  </span>
                </span>
                <span
                  className={`font-inter text-[10px] uppercase tracking-[0.16em] ${
                    active ? 'text-white/80' : 'text-[#A09890]'
                  }`}
                >
                  {connecting
                    ? copy.status.opening
                    : active
                      ? copy.status.connected
                      : copy.status.add}
                </span>
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}

async function loadGraphPreview(): Promise<PreviewResponse | null> {
  for (let attempt = 0; attempt <= GRAPH_PREVIEW_RETRY_DELAYS_MS.length; attempt += 1) {
    try {
      const response = await fetch('/api/onboarding/graph-preview', {
        method: 'GET',
        credentials: 'include',
        cache: 'no-store',
      })
      if (response.ok) {
        return (await response.json()) as PreviewResponse
      }
      if (!isTransientGraphPreviewStatus(response.status)) return null
    } catch {
      // Retry below. If every attempt fails, keep the last rendered graph.
    }

    const delayMs = GRAPH_PREVIEW_RETRY_DELAYS_MS[attempt]
    if (delayMs == null) break
    await wait(delayMs)
  }
  return null
}

function isTransientGraphPreviewStatus(status: number): boolean {
  return status === 404 || status === 502 || status === 503 || status === 504
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

function IntegrationConnectOverlay({
  frame,
  iframeRef,
  copy,
  onClose,
}: {
  frame: ConnectFrameState
  iframeRef: React.RefObject<HTMLIFrameElement | null>
  copy: ConnectOverlayCopy
  onClose: () => void
}) {
  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center rounded-[24px] bg-[#F5F4F2]/90 p-4 backdrop-blur-md sm:p-6">
      <div className="relative h-[min(860px,calc(100dvh-110px))] w-full max-w-[760px] overflow-hidden rounded-[28px] border border-[#D6D2CB] bg-white shadow-[0_28px_90px_rgba(17,17,17,0.22)]">
        <div className="flex h-[76px] items-center justify-between border-b border-[#E7E5E4] px-7">
          <div>
            <p className="font-inter text-[13px] uppercase tracking-[0.28em] text-[#A09890]">
              {copy.eyebrow}
            </p>
            <p className="mt-1 font-inter text-[20px] font-semibold text-[#1F1B17]">
              {frame.connector.label}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={copy.close}
            title={copy.close}
            className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-[#D6D2CB] bg-white text-[#1F1B17] shadow-sm transition-colors hover:border-[#A09890]"
          >
            <X className="h-6 w-6" strokeWidth={2} />
          </button>
        </div>
        <iframe
          ref={iframeRef}
          src={frame.url}
          title={formatOnboardingText(copy.title, {
            label: frame.connector.label,
          })}
          className="h-[calc(100%-76px)] w-full bg-[#080808]"
          allow="clipboard-write; popups; popups-to-escape-sandbox"
        />
      </div>
    </div>
  )
}

/**
 * Render the live graph. Layout is deterministic-radial: every node
 * id hashes into a stable angle/radius around the org anchor so a
 * poll refresh doesn't shuffle the canvas. New nodes briefly glow
 * green for one render cycle to highlight that they just arrived.
 */
function GraphReveal({
  graph,
  warning,
  sourceCount,
  countsTemplate,
}: {
  graph: PreviewResponse | null
  warning: string | null
  sourceCount: number
  countsTemplate: string
}) {
  const previousIdsRef = useRef<Set<string>>(new Set())
  const [highlightedIds, setHighlightedIds] = useState<Set<string>>(new Set())
  const displayGraph = useMemo(() => normalisePreviewGraph(graph), [graph])

  useEffect(() => {
    if (!displayGraph) return
    const currentIds = new Set(displayGraph.nodes.map((n) => n.id))
    const newOnes = new Set<string>()
    for (const id of currentIds) {
      if (!previousIdsRef.current.has(id)) newOnes.add(id)
    }
    previousIdsRef.current = currentIds
    if (newOnes.size === 0) return
    const showTimer = window.setTimeout(() => setHighlightedIds(newOnes), 0)
    const clearTimer = window.setTimeout(() => setHighlightedIds(new Set()), 1_200)
    return () => {
      window.clearTimeout(showTimer)
      window.clearTimeout(clearTimer)
    }
  }, [displayGraph])

  const nodes = displayGraph?.nodes ?? EMPTY_PREVIEW_NODES
  const layout = useMemo(() => layoutNodes(nodes), [nodes])
  const edges = displayGraph?.edges ?? []
  const showDenseLabels = nodes.length <= 24

  return (
    <div className="relative flex h-full w-full items-center justify-center bg-[#0F0F10] p-8">
      <svg
        viewBox="0 0 400 400"
        className="h-[80%] w-[80%]"
        role="presentation"
        aria-hidden="true"
      >
        {edges.map((edge, i) => {
          const a = layout.get(edge.a)
          const b = layout.get(edge.b)
          if (!a || !b) return null
          return (
            <line
              key={`${edge.a}-${edge.b}-${i}`}
              x1={a.cx}
              y1={a.cy}
              x2={b.cx}
              y2={b.cy}
              stroke="#3B3B3D"
              strokeWidth={0.7}
              strokeOpacity={0.7}
            />
          )
        })}
        {nodes.map((node) => {
          const pos = layout.get(node.id)
          if (!pos) return null
          const isOrg = node.group === 'org'
          const showLabel =
            showDenseLabels ||
            isOrg ||
            node.group === 'integration' ||
            node.group === 'source'
          const justArrived = highlightedIds.has(node.id)
          const fill = isOrg
            ? '#F5E5A8'
            : justArrived
              ? '#34D399'
              : groupColour(node.group)
          return (
            <g key={node.id}>
              <circle
                cx={pos.cx}
                cy={pos.cy}
                r={isOrg ? 6 : justArrived ? 4.5 : 3.2}
                fill={fill}
                stroke={justArrived ? '#34D39955' : 'transparent'}
                strokeWidth={justArrived ? 5 : 0}
              />
              {showLabel && (
                <text
                  x={pos.cx}
                  y={pos.cy + 18}
                  textAnchor="middle"
                  className="font-mono"
                  fontSize={isOrg ? 9 : 7.5}
                  fill={isOrg ? '#E5DFD3' : '#B8B2A8'}
                >
                  {truncateNodeLabel(node.label)}
                </text>
              )}
            </g>
          )
        })}
      </svg>
      {warning && (
        <div className="pointer-events-none absolute left-6 right-6 top-6 rounded-lg border border-white/10 bg-black/45 px-3 py-2 backdrop-blur">
          <p className="font-inter text-[10px] leading-4 text-white/70">
            {warning}
          </p>
        </div>
      )}
      <div className="pointer-events-none absolute bottom-6 right-6 rounded-lg border border-white/10 bg-black/40 px-3 py-2 backdrop-blur">
        <p className="font-mono text-[10px] tabular-nums text-white/80">
          {formatOnboardingText(countsTemplate, {
            sources: sourceCount,
            nodes: displayGraph?.counts.nodes ?? 0,
            edges: displayGraph?.counts.edges ?? 0,
          })}
        </p>
      </div>
    </div>
  )
}

interface NodePos {
  cx: number
  cy: number
}

function layoutNodes(nodes: PreviewNode[]): Map<string, NodePos> {
  const out = new Map<string, NodePos>()
  for (const node of nodes) {
    if (node.group === 'org') {
      out.set(node.id, { cx: 200, cy: 200 })
      continue
    }
    const seed = hashId(node.id)
    const angle = (seed % 360) * (Math.PI / 180)
    const radius = 60 + ((seed >> 8) % 110)
    out.set(node.id, {
      cx: 200 + Math.cos(angle) * radius,
      cy: 200 + Math.sin(angle) * radius,
    })
  }
  return out
}

function hashId(input: string): number {
  let hash = 0
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 31 + input.charCodeAt(i)) | 0
  }
  return Math.abs(hash)
}

function truncateNodeLabel(label: string): string {
  return label.length > 18 ? `${label.slice(0, 17)}…` : label
}

function normalisePreviewGraph(graph: PreviewResponse | null): PreviewResponse | null {
  if (!graph) return null

  const nodesById = new Map<string, PreviewNode>()
  for (const node of graph.nodes) {
    if (!node.id || nodesById.has(node.id)) continue
    nodesById.set(node.id, node)
  }

  const edgesByKey = new Map<string, PreviewEdge>()
  for (const edge of graph.edges) {
    if (!nodesById.has(edge.a) || !nodesById.has(edge.b)) continue
    const key = `${edge.a}\u0000${edge.b}`
    if (edgesByKey.has(key)) continue
    edgesByKey.set(key, edge)
  }

  const nodes = Array.from(nodesById.values())
  const edges = Array.from(edgesByKey.values())
  const groupSet = new Set(
    nodes.filter((node) => node.group !== 'org').map((node) => node.group),
  )

  return {
    nodes,
    edges,
    counts: {
      nodes: nodes.length,
      edges: edges.length,
      groups: groupSet.size,
    },
    warning: graph.warning,
  }
}

function withWebsiteSource(
  graph: PreviewResponse | null,
  website?: WebsitePayload,
  organization?: OrganizationPayload,
): PreviewResponse | null {
  const url = website?.url?.trim()
  if (!url) return graph

  const nodes = [...(graph?.nodes ?? [])]
  const edges = [...(graph?.edges ?? [])]
  const nodeIds = new Set(nodes.map((node) => node.id))
  let orgNodeId = nodes.find((node) => node.group === 'org')?.id
  if (!orgNodeId) {
    orgNodeId = 'org'
    nodes.push({
      id: orgNodeId,
      label: organization?.id?.slice(0, 8) || organization?.name || 'Org',
      group: 'org',
    })
    nodeIds.add(orgNodeId)
  }

  const websiteNodeId = `website:${safeSourceIdPart(url)}`
  if (!nodeIds.has(websiteNodeId)) {
    nodes.push({
      id: websiteNodeId,
      label: websiteHost(url),
      group: 'source',
    })
    nodeIds.add(websiteNodeId)
  }
  if (!edges.some((edge) => edge.a === orgNodeId && edge.b === websiteNodeId)) {
    edges.push({ a: orgNodeId, b: websiteNodeId })
  }

  const groupSet = new Set(
    nodes.filter((node) => node.group !== 'org').map((node) => node.group),
  )
  return normalisePreviewGraph({
    nodes,
    edges,
    counts: {
      nodes: nodes.length,
      edges: edges.length,
      groups: groupSet.size,
    },
    warning: graph?.warning,
  })
}

function websiteHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

function safeSourceIdPart(input: string): string {
  return input.replace(/[^A-Za-z0-9_-]/g, '_').slice(0, 64) || 'source'
}

function toEmbeddedConnectUrl(input: string): string {
  try {
    const url = new URL(input)
    url.searchParams.set('embedded', 'true')
    url.searchParams.set('detectClosedAuthWindow', 'true')
    return url.toString()
  } catch {
    const separator = input.includes('?') ? '&' : '?'
    return `${input}${separator}embedded=true&detectClosedAuthWindow=true`
  }
}

function parseNangoConnectMessage(value: unknown): NangoConnectMessage | null {
  if (typeof value === 'string') {
    try {
      return parseNangoConnectMessage(JSON.parse(value) as unknown)
    } catch {
      return null
    }
  }
  if (!value || typeof value !== 'object') return null
  const record = value as {
    type?: unknown
    event?: unknown
    name?: unknown
    payload?: unknown
    data?: unknown
  }
  const rawType = [record.type, record.event, record.name]
    .find((candidate) => typeof candidate === 'string') as string | undefined
  if (!rawType) return null
  const type = rawType.toLowerCase()
  const payload = record.payload ?? record.data
  if (type.includes('ready')) return { type: 'ready', payload }
  if (
    type.includes('success') ||
    type.includes('complete') ||
    type === 'connect' ||
    type.endsWith(':connect')
  ) {
    return { type: 'connect', payload }
  }
  if (type.includes('error') || type.includes('fail')) {
    return { type: 'error', payload }
  }
  if (type.includes('close')) return { type: 'close', payload }
  return null
}

function connectMessageError(payload: unknown, fallback: string): string {
  if (!payload || typeof payload !== 'object') {
    return fallback
  }
  const record = payload as { error?: unknown; message?: unknown }
  if (typeof record.message === 'string' && record.message.trim()) {
    return record.message
  }
  if (typeof record.error === 'string' && record.error.trim()) {
    return record.error
  }
  return fallback
}

function normalizeConnectorId(id: string): string {
  return MICROSOFT_CONNECTOR_ALIASES.has(id) ? 'microsoft365' : id
}

function connectorStateIds(id: string): string[] {
  if (!MICROSOFT_CONNECTOR_ALIASES.has(id)) return [id]
  return ['microsoft365', 'teams', 'sharepoint', 'onedrive', 'outlook', 'm365']
}

// Colour-code clusters by group. Unknown graph entity types fall back
// to neutral mid-grey so upstream schema additions still render.
function groupColour(group: string): string {
  switch (group) {
    case 'integration':
      return '#F5E5A8'
    case 'source':
      return '#34D399'
    case 'person':
    case 'user':
      return '#9BD0E8'
    case 'product':
      return '#F0A8A1'
    case 'document':
    case 'file':
    case 'folder':
      return '#C7B0F0'
    case 'email':
      return '#9BD0E8'
    case 'channel':
    case 'team':
      return '#A8E0B6'
    case 'repository':
      return '#F4C16D'
    case 'contact':
    case 'customer':
      return '#F0A8A1'
    default:
      return '#5B5B5C'
  }
}
