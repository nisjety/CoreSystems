'use client'

/**
 * Step 4 — knowledge connectors with live Obsidian-style graph.
 *
 * Left pane: connector picker (Slack / Notion / Google Drive /
 * SharePoint / Zammad). Clicking a connector marks it as picked and
 * adds a clutch of nodes to the graph on the right. Skip ahead is
 * always available.
 *
 * Right pane: force-directed graph reveal (Slot 4 in the prompts MD).
 * Each connector contributes 6–10 nodes; edges are drawn live as the
 * Data Plane graph-index processes the source. The animation is
 * pure-SVG / pure-CSS to avoid bringing in a heavy viz dep — the
 * Velion bundle is already large enough.
 */

import React, { useMemo, useState } from 'react'

import type { OnboardingMachine } from '../state/useOnboardingMachine'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

const CONNECTORS = [
  { id: 'slack', label: 'Slack', hint: 'Kanaler + tråder' },
  { id: 'notion', label: 'Notion', hint: 'Sider + databaser' },
  { id: 'gdrive', label: 'Google Drive', hint: 'Docs + Sheets' },
  { id: 'sharepoint', label: 'SharePoint', hint: 'Tenants + sites' },
  { id: 'zammad', label: 'Zammad', hint: 'Tickets + KB' },
  { id: 'github', label: 'GitHub', hint: 'README + issues' },
] as const

interface Node {
  id: string
  cx: number
  cy: number
  group: string
  /** Set briefly to true when a node is added so it glows green. */
  highlight: boolean
}

interface Edge {
  a: string
  b: string
}

export function ConnectStep({ machine }: { machine: OnboardingMachine }) {
  const initial = machine.state.connectors
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(initial.map((c) => c.id)),
  )
  const [graph, setGraph] = useState<{ nodes: Node[]; edges: Edge[] }>({
    nodes: seedNodes(initial.map((c) => c.id)),
    edges: seedEdges(initial.map((c) => c.id)),
  })

  const toggle = (id: string, label: string) => {
    if (picked.has(id)) {
      machine.removeConnector(id)
      setPicked((prev) => {
        const next = new Set(prev)
        next.delete(id)
        return next
      })
      setGraph((prev) => removeGroupFromGraph(prev, id))
      return
    }
    machine.addConnector({ id, label, authedAt: new Date().toISOString() })
    setPicked((prev) => new Set([...prev, id]))
    setGraph((prev) => addGroupToGraph(prev, id))
  }

  const submit = () => {
    machine.goTo('social-proof')
  }

  const counters = useMemo(
    () => ({
      nodes: graph.nodes.length,
      edges: graph.edges.length,
      sources: picked.size,
    }),
    [graph, picked],
  )

  return (
    <>
      <LeftPane>
        <StepEyebrow>Steg 4 av 6</StepEyebrow>
        <StepTitle>Koble til kunnskap.</StepTitle>
        <StepDescription>
          Velg kildene Velion skal lære av. Hver kilde havner i
          kunnskaps­grafen på høyre side mens vi henter innholdet.
        </StepDescription>

        <ul className="flex flex-col gap-2">
          {CONNECTORS.map((c) => {
            const active = picked.has(c.id)
            return (
              <li key={c.id}>
                <button
                  type="button"
                  onClick={() => toggle(c.id, c.label)}
                  className={`flex w-full items-center justify-between rounded-md border px-4 py-3 text-left transition-colors ${
                    active
                      ? 'border-[#1F1B17] bg-[#1F1B17] text-white'
                      : 'border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]'
                  }`}
                >
                  <span>
                    <span className="block font-inter text-[13px] font-medium">
                      {c.label}
                    </span>
                    <span
                      className={`mt-0.5 block font-inter text-[11px] ${active ? 'text-white/70' : 'text-[#A09890]'}`}
                    >
                      {c.hint}
                    </span>
                  </span>
                  <span
                    className={`font-inter text-[11px] uppercase tracking-[0.16em] ${active ? 'text-white/80' : 'text-[#A09890]'}`}
                  >
                    {active ? 'Lagt til' : 'Legg til'}
                  </span>
                </button>
              </li>
            )
          })}
        </ul>

        <div className="mt-2 flex items-center gap-4">
          <PrimaryButton onClick={submit}>Fortsett</PrimaryButton>
          <SkipLink onClick={submit}>Hopp over</SkipLink>
        </div>
      </LeftPane>

      <RightPane>
        <GraphReveal graph={graph} counters={counters} />
      </RightPane>
    </>
  )
}

function GraphReveal({
  graph,
  counters,
}: {
  graph: { nodes: Node[]; edges: Edge[] }
  counters: { nodes: number; edges: number; sources: number }
}) {
  return (
    <div className="relative flex h-full w-full items-center justify-center bg-[#0F0F10] p-8">
      <svg
        viewBox="0 0 400 400"
        className="h-[80%] w-[80%]"
        role="presentation"
        aria-hidden="true"
      >
        {graph.edges.map((edge) => {
          const a = graph.nodes.find((n) => n.id === edge.a)
          const b = graph.nodes.find((n) => n.id === edge.b)
          if (!a || !b) return null
          return (
            <line
              key={`${edge.a}-${edge.b}`}
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
        {graph.nodes.map((node) => (
          <circle
            key={node.id}
            cx={node.cx}
            cy={node.cy}
            r={node.highlight ? 4 : 2.8}
            fill={node.highlight ? '#34D399' : '#5B5B5C'}
            stroke={node.highlight ? '#34D39955' : 'transparent'}
            strokeWidth={node.highlight ? 4 : 0}
          />
        ))}
      </svg>
      <div className="pointer-events-none absolute bottom-6 right-6 rounded-lg border border-white/10 bg-black/40 px-3 py-2 backdrop-blur">
        <p className="font-mono text-[10px] tabular-nums text-white/80">
          nodes {counters.nodes} · edges {counters.edges} · kilder {counters.sources}
        </p>
      </div>
    </div>
  )
}

// ─────────────────────────────────────────────────────────────────────
// Graph helpers
// ─────────────────────────────────────────────────────────────────────

function seedNodes(picked: string[]): Node[] {
  const base: Node[] = []
  // The org sits at the centre of the canvas as the anchor node.
  base.push({ id: 'org', cx: 200, cy: 200, group: 'org', highlight: false })
  for (const id of picked) {
    base.push(...nodesForGroup(id, base.length))
  }
  return base
}

function seedEdges(picked: string[]): Edge[] {
  const edges: Edge[] = []
  for (const id of picked) {
    edges.push(...edgesForGroup(id))
  }
  return edges
}

function nodesForGroup(group: string, offset: number): Node[] {
  const count = 7
  return Array.from({ length: count }).map((_, i) => {
    const angle = (offset + i) * 0.7
    const radius = 60 + ((offset + i) % 4) * 25
    return {
      id: `${group}-${i}`,
      cx: 200 + Math.cos(angle) * radius,
      cy: 200 + Math.sin(angle) * radius,
      group,
      highlight: false,
    }
  })
}

function edgesForGroup(group: string): Edge[] {
  const edges: Edge[] = []
  for (let i = 0; i < 7; i += 1) {
    edges.push({ a: 'org', b: `${group}-${i}` })
    if (i > 0) edges.push({ a: `${group}-${i - 1}`, b: `${group}-${i}` })
  }
  return edges
}

function addGroupToGraph(
  prev: { nodes: Node[]; edges: Edge[] },
  group: string,
): { nodes: Node[]; edges: Edge[] } {
  const newNodes = nodesForGroup(group, prev.nodes.length).map((n) => ({
    ...n,
    highlight: true,
  }))
  return {
    nodes: [...prev.nodes, ...newNodes],
    edges: [...prev.edges, ...edgesForGroup(group)],
  }
}

function removeGroupFromGraph(
  prev: { nodes: Node[]; edges: Edge[] },
  group: string,
): { nodes: Node[]; edges: Edge[] } {
  return {
    nodes: prev.nodes.filter((n) => n.group !== group),
    edges: prev.edges.filter(
      (e) => !e.a.startsWith(`${group}-`) && !e.b.startsWith(`${group}-`),
    ),
  }
}
