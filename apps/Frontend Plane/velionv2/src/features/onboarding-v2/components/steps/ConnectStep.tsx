"use client";

/**
 * Step 4 — knowledge connectors with live graph reveal.
 *
 * Left: grouped source picker (Chat / Docs / Tools). Clicking a row creates a
 * Nango connect session via the BFF (org + user resolved server-side from the
 * session), embeds Nango Connect in an overlay, and records the pick only after
 * Nango reports success.
 *
 * Right: deterministic-radial node-link graph driven by graph-index-rs, polled
 * every 5s via React Query (dedup + cancellation handled by the query client).
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  createConnectSession,
  fetchGraphPreview,
  type PreviewEdge,
  type PreviewNode,
  type PreviewResponse,
} from "../../lib/onboarding-api";
import {
  formatOnboardingText,
  type OnboardingLocale,
  useOnboardingCopy,
} from "../../lib/onboarding-i18n";
import type {
  OnboardingMachine,
  OrganizationPayload,
  WebsitePayload,
} from "../../lib/onboarding-machine";
import {
  LeftPane,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

interface ConnectorMeta {
  id: string;
  label: string;
  hint: string;
  category: "chat" | "docs" | "tools";
  provider: string;
  sources: string[];
}

const CONNECTORS: ConnectorMeta[] = [
  { id: "slack", label: "Slack", hint: "Channels + threads", category: "chat", provider: "slack", sources: ["messages"] },
  { id: "microsoft365", label: "Microsoft 365", hint: "Teams, Outlook, SharePoint, OneDrive", category: "chat", provider: "microsoft", sources: ["teams", "outlook", "sharepoint", "onedrive"] },
  { id: "notion", label: "Notion", hint: "Pages + databases", category: "docs", provider: "notion", sources: ["pages", "databases"] },
  { id: "gdrive", label: "Google Drive", hint: "Docs + Sheets", category: "docs", provider: "google-drive", sources: ["google_drive", "documents"] },
  { id: "github", label: "GitHub", hint: "README + issues", category: "tools", provider: "github", sources: ["issues"] },
];

const CONNECTOR_HINTS: Record<OnboardingLocale, Record<string, string>> = {
  nb: { slack: "Kanaler + tråder", microsoft365: "Teams, Outlook, SharePoint, OneDrive", notion: "Sider + databaser", gdrive: "Dokumenter + regneark", github: "README + issues" },
  en: { slack: "Channels + threads", microsoft365: "Teams, Outlook, SharePoint, OneDrive", notion: "Pages + databases", gdrive: "Docs + Sheets", github: "README + issues" },
};

const GRAPH_POLL_INTERVAL_MS = 5_000;
const EMPTY_NODES: PreviewNode[] = [];
const MICROSOFT_ALIASES = new Set(["teams", "sharepoint", "onedrive", "outlook", "m365", "microsoft365", "microsoft-365"]);

interface ConnectFrameState {
  connector: ConnectorMeta;
  url: string;
}

interface NangoConnectMessage {
  type: "ready" | "connect" | "error" | "close";
  payload?: unknown;
}

export function ConnectStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy } = useOnboardingCopy();
  const queryClient = useQueryClient();
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(machine.state.connectors.map((c) => normalizeConnectorId(c.id))),
  );
  const [connectError, setConnectError] = useState<string | null>(null);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectFrame, setConnectFrame] = useState<ConnectFrameState | null>(null);
  const iframeRef = useRef<HTMLIFrameElement | null>(null);

  const { data: graph } = useQuery({
    queryKey: ["onboarding", "graph-preview"],
    queryFn: ({ signal }) => fetchGraphPreview(signal),
    refetchInterval: GRAPH_POLL_INTERVAL_MS,
    staleTime: 0,
  });

  const nudgeGraph = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ["onboarding", "graph-preview"] });
  }, [queryClient]);

  const completeConnector = useCallback(
    (connector: ConnectorMeta) => {
      machine.addConnector({ id: connector.id, label: connector.label, authedAt: new Date().toISOString() });
      setPicked((prev) => (prev.has(connector.id) ? prev : new Set([...prev, connector.id])));
      setConnectFrame(null);
      setConnectingId(null);
      setConnectError(null);
      nudgeGraph();
    },
    [machine, nudgeGraph],
  );

  const toggle = useCallback(
    async (connector: ConnectorMeta) => {
      if (picked.has(connector.id)) {
        connectorStateIds(connector.id).forEach((id) => machine.removeConnector(id));
        setPicked((prev) => {
          const next = new Set(prev);
          next.delete(connector.id);
          return next;
        });
        setConnectError(null);
        nudgeGraph();
        return;
      }
      setConnectingId(connector.id);
      setConnectError(null);
      try {
        const session = await createConnectSession({ provider: connector.provider, sources: connector.sources });
        setConnectFrame({ connector, url: toEmbeddedConnectUrl(session.connectUrl) });
      } catch (error) {
        setConnectError(
          error instanceof Error
            ? error.message
            : formatOnboardingText(copy.connect.couldNotConnect, { label: connector.label }),
        );
        setConnectingId(null);
      }
    },
    [copy.connect, machine, nudgeGraph, picked],
  );

  useEffect(() => {
    if (!connectFrame) return;
    const handleMessage = (event: MessageEvent) => {
      if (event.source !== iframeRef.current?.contentWindow) return;
      const message = parseNangoConnectMessage(event.data);
      if (!message) return;
      if (message.type === "connect") {
        completeConnector(connectFrame.connector);
      } else if (message.type === "error") {
        setConnectError(connectMessageError(message.payload, copy.connect.connectFlowFailed));
        setConnectFrame(null);
        setConnectingId(null);
      } else if (message.type === "close") {
        setConnectFrame(null);
        setConnectingId(null);
      }
    };
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, [completeConnector, connectFrame, copy.connect.connectFlowFailed]);

  const submit = () => machine.goTo("social-proof");

  const sourceCount = picked.size + (machine.state.website?.url ? 1 : 0);
  const previewGraph = useMemo(
    () => withWebsiteSource(graph ?? null, machine.state.website, machine.state.organization),
    [graph, machine.state.organization, machine.state.website],
  );
  const counts = previewGraph?.counts ?? { nodes: 0, edges: 0, groups: 0 };

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.connect.eyebrow}</StepEyebrow>
        <StepTitle>{copy.connect.title}</StepTitle>
        <StepDescription>{copy.connect.description}</StepDescription>

        <div className="flex flex-col gap-5">
          {(["chat", "docs", "tools"] as const).map((category) => (
            <SourceCategory
              key={category}
              title={copy.connect.categories[category]}
              items={CONNECTORS.filter((c) => c.category === category)}
              picked={picked}
              connectingId={connectingId}
              onToggle={toggle}
              locale={locale}
              statusCopy={copy.connect.status}
            />
          ))}
        </div>

        {connectError && <p className="font-inter text-[12px] leading-5 text-[#B42318]">{connectError}</p>}

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
          warning={previewGraph?.warning ?? null}
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
            setConnectFrame(null);
            setConnectingId(null);
          }}
        />
      )}
    </>
  );
}

function SourceCategory({
  title,
  items,
  picked,
  connectingId,
  onToggle,
  locale,
  statusCopy,
}: {
  title: string;
  items: ConnectorMeta[];
  picked: Set<string>;
  connectingId: string | null;
  onToggle: (connector: ConnectorMeta) => void;
  locale: OnboardingLocale;
  statusCopy: { opening: string; connected: string; add: string };
}) {
  return (
    <div>
      <p className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">{title}</p>
      <ul className="mt-2 flex flex-col gap-1.5">
        {items.map((c) => {
          const active = picked.has(c.id);
          const connecting = connectingId === c.id;
          return (
            <li key={c.id}>
              <button
                type="button"
                onClick={() => onToggle(c)}
                aria-pressed={active}
                disabled={connectingId !== null}
                className={cn(
                  "flex w-full items-center justify-between rounded-md border px-3.5 py-2.5 text-left transition-colors",
                  active
                    ? "border-[#1F1B17] bg-[#1F1B17] text-white"
                    : "border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890] disabled:cursor-wait disabled:opacity-60",
                )}
              >
                <span className="flex flex-col">
                  <span className="font-inter text-[12.5px]">{c.label}</span>
                  <span className={cn("mt-0.5 font-inter text-[11px]", active ? "text-white/70" : "text-[#6B6660]")}>
                    {CONNECTOR_HINTS[locale][c.id] ?? c.hint}
                  </span>
                </span>
                <span className={cn("font-inter text-[10px] uppercase tracking-[0.16em]", active ? "text-white/80" : "text-[#A09890]")}>
                  {connecting ? statusCopy.opening : active ? statusCopy.connected : statusCopy.add}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function IntegrationConnectOverlay({
  frame,
  iframeRef,
  copy,
  onClose,
}: {
  frame: ConnectFrameState;
  iframeRef: React.RefObject<HTMLIFrameElement | null>;
  copy: { eyebrow: string; close: string; title: string };
  onClose: () => void;
}) {
  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center rounded-[24px] bg-[#F5F4F2]/90 p-4 backdrop-blur-md sm:p-6">
      <div className="relative h-[min(860px,calc(100dvh-110px))] w-full max-w-[760px] overflow-hidden rounded-[28px] border border-[#D6D2CB] bg-white shadow-[0_28px_90px_rgba(17,17,17,0.22)]">
        <div className="flex h-[76px] items-center justify-between border-b border-[#E7E5E4] px-7">
          <div>
            <p className="font-inter text-[13px] uppercase tracking-[0.28em] text-[#A09890]">{copy.eyebrow}</p>
            <p className="mt-1 font-inter text-[20px] font-semibold text-[#1F1B17]">{frame.connector.label}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={copy.close}
            title={copy.close}
            className="inline-flex size-12 items-center justify-center rounded-full border border-[#D6D2CB] bg-white text-[#1F1B17] shadow-sm transition-colors hover:border-[#A09890]"
          >
            <X className="size-6" strokeWidth={2} />
          </button>
        </div>
        <iframe
          ref={iframeRef}
          src={frame.url}
          title={formatOnboardingText(copy.title, { label: frame.connector.label })}
          className="h-[calc(100%-76px)] w-full bg-[#080808]"
          allow="clipboard-write; popups; popups-to-escape-sandbox"
        />
      </div>
    </div>
  );
}

/* ----------------------------------------------------------- graph render */

function GraphReveal({
  graph,
  warning,
  sourceCount,
  countsTemplate,
}: {
  graph: PreviewResponse | null;
  warning: string | null;
  sourceCount: number;
  countsTemplate: string;
}) {
  const previousIdsRef = useRef<Set<string>>(new Set());
  const [highlightedIds, setHighlightedIds] = useState<Set<string>>(new Set());
  const displayGraph = useMemo(() => normalisePreviewGraph(graph), [graph]);

  useEffect(() => {
    if (!displayGraph) return;
    const currentIds = new Set(displayGraph.nodes.map((n) => n.id));
    const newOnes = new Set<string>();
    for (const id of currentIds) if (!previousIdsRef.current.has(id)) newOnes.add(id);
    previousIdsRef.current = currentIds;
    if (newOnes.size === 0) return;
    const showTimer = window.setTimeout(() => setHighlightedIds(newOnes), 0);
    const clearTimer = window.setTimeout(() => setHighlightedIds(new Set()), 1_200);
    return () => {
      window.clearTimeout(showTimer);
      window.clearTimeout(clearTimer);
    };
  }, [displayGraph]);

  const nodes = displayGraph?.nodes ?? EMPTY_NODES;
  const layout = useMemo(() => layoutNodes(nodes), [nodes]);
  const edges = displayGraph?.edges ?? [];
  const showDenseLabels = nodes.length <= 24;

  return (
    <div className="relative flex size-full items-center justify-center bg-[#0F0F10] p-8">
      <svg viewBox="0 0 400 400" className="h-[80%] w-[80%]" role="presentation" aria-hidden="true">
        {edges.map((edge, i) => {
          const a = layout.get(edge.a);
          const b = layout.get(edge.b);
          if (!a || !b) return null;
          return <line key={`${edge.a}-${edge.b}-${i}`} x1={a.cx} y1={a.cy} x2={b.cx} y2={b.cy} stroke="#3B3B3D" strokeWidth={0.7} strokeOpacity={0.7} />;
        })}
        {nodes.map((node) => {
          const pos = layout.get(node.id);
          if (!pos) return null;
          const isOrg = node.group === "org";
          const showLabel = showDenseLabels || isOrg || node.group === "integration" || node.group === "source";
          const justArrived = highlightedIds.has(node.id);
          const fill = isOrg ? "#F5E5A8" : justArrived ? "#34D399" : groupColour(node.group);
          return (
            <g key={node.id}>
              <circle
                cx={pos.cx}
                cy={pos.cy}
                r={isOrg ? 6 : justArrived ? 4.5 : 3.2}
                fill={fill}
                stroke={justArrived ? "#34D39955" : "transparent"}
                strokeWidth={justArrived ? 5 : 0}
              />
              {showLabel && (
                <text x={pos.cx} y={pos.cy + 18} textAnchor="middle" className="font-mono" fontSize={isOrg ? 9 : 7.5} fill={isOrg ? "#E5DFD3" : "#B8B2A8"}>
                  {truncateNodeLabel(node.label)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      {warning && (
        <div className="pointer-events-none absolute inset-x-6 top-6 rounded-lg border border-white/10 bg-black/45 px-3 py-2 backdrop-blur">
          <p className="font-inter text-[10px] leading-4 text-white/70">{warning}</p>
        </div>
      )}
      <div className="absolute bottom-6 right-6 rounded-lg border border-white/10 bg-black/40 px-3 py-2 backdrop-blur" aria-live="polite">
        <p className="font-mono text-[10px] tabular-nums text-white/80">
          {formatOnboardingText(countsTemplate, {
            sources: sourceCount,
            nodes: displayGraph?.counts.nodes ?? 0,
            edges: displayGraph?.counts.edges ?? 0,
          })}
        </p>
      </div>
    </div>
  );
}

interface NodePos {
  cx: number;
  cy: number;
}

function layoutNodes(nodes: PreviewNode[]): Map<string, NodePos> {
  const out = new Map<string, NodePos>();
  for (const node of nodes) {
    if (node.group === "org") {
      out.set(node.id, { cx: 200, cy: 200 });
      continue;
    }
    const seed = hashId(node.id);
    const angle = (seed % 360) * (Math.PI / 180);
    const radius = 60 + ((seed >> 8) % 110);
    out.set(node.id, { cx: 200 + Math.cos(angle) * radius, cy: 200 + Math.sin(angle) * radius });
  }
  return out;
}

function hashId(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) hash = (hash * 31 + input.charCodeAt(i)) | 0;
  return Math.abs(hash);
}

function truncateNodeLabel(label: string): string {
  return label.length > 18 ? `${label.slice(0, 17)}…` : label;
}

function normalisePreviewGraph(graph: PreviewResponse | null): PreviewResponse | null {
  if (!graph) return null;
  const nodesById = new Map<string, PreviewNode>();
  for (const node of graph.nodes) {
    if (!node.id || nodesById.has(node.id)) continue;
    nodesById.set(node.id, node);
  }
  const edgesByKey = new Map<string, PreviewEdge>();
  for (const edge of graph.edges) {
    if (!nodesById.has(edge.a) || !nodesById.has(edge.b)) continue;
    const key = `${edge.a} ${edge.b}`;
    if (edgesByKey.has(key)) continue;
    edgesByKey.set(key, edge);
  }
  const nodes = Array.from(nodesById.values());
  const edges = Array.from(edgesByKey.values());
  const groups = new Set(nodes.filter((n) => n.group !== "org").map((n) => n.group));
  return { nodes, edges, counts: { nodes: nodes.length, edges: edges.length, groups: groups.size }, warning: graph.warning };
}

function withWebsiteSource(
  graph: PreviewResponse | null,
  website?: WebsitePayload,
  organization?: OrganizationPayload,
): PreviewResponse | null {
  const url = website?.url?.trim();
  if (!url) return graph;
  const nodes = [...(graph?.nodes ?? [])];
  const edges = [...(graph?.edges ?? [])];
  const nodeIds = new Set(nodes.map((n) => n.id));
  let orgNodeId = nodes.find((n) => n.group === "org")?.id;
  if (!orgNodeId) {
    orgNodeId = "org";
    nodes.push({ id: orgNodeId, label: organization?.id?.slice(0, 8) || organization?.name || "Org", group: "org" });
    nodeIds.add(orgNodeId);
  }
  const websiteNodeId = `website:${safeSourceIdPart(url)}`;
  if (!nodeIds.has(websiteNodeId)) {
    nodes.push({ id: websiteNodeId, label: websiteHost(url), group: "source" });
    nodeIds.add(websiteNodeId);
  }
  if (!edges.some((e) => e.a === orgNodeId && e.b === websiteNodeId)) edges.push({ a: orgNodeId, b: websiteNodeId });
  const groups = new Set(nodes.filter((n) => n.group !== "org").map((n) => n.group));
  return normalisePreviewGraph({ nodes, edges, counts: { nodes: nodes.length, edges: edges.length, groups: groups.size }, warning: graph?.warning });
}

function websiteHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function safeSourceIdPart(input: string): string {
  return input.replace(/[^A-Za-z0-9_-]/g, "_").slice(0, 64) || "source";
}

function toEmbeddedConnectUrl(input: string): string {
  try {
    const url = new URL(input);
    url.searchParams.set("embedded", "true");
    url.searchParams.set("detectClosedAuthWindow", "true");
    return url.toString();
  } catch {
    const sep = input.includes("?") ? "&" : "?";
    return `${input}${sep}embedded=true&detectClosedAuthWindow=true`;
  }
}

function parseNangoConnectMessage(value: unknown): NangoConnectMessage | null {
  if (typeof value === "string") {
    try {
      return parseNangoConnectMessage(JSON.parse(value) as unknown);
    } catch {
      return null;
    }
  }
  if (!value || typeof value !== "object") return null;
  const record = value as { type?: unknown; event?: unknown; name?: unknown; payload?: unknown; data?: unknown };
  const rawType = [record.type, record.event, record.name].find((c) => typeof c === "string") as string | undefined;
  if (!rawType) return null;
  const type = rawType.toLowerCase();
  const payload = record.payload ?? record.data;
  if (type.includes("ready")) return { type: "ready", payload };
  if (type.includes("success") || type.includes("complete") || type === "connect" || type.endsWith(":connect")) {
    return { type: "connect", payload };
  }
  if (type.includes("error") || type.includes("fail")) return { type: "error", payload };
  if (type.includes("close")) return { type: "close", payload };
  return null;
}

function connectMessageError(payload: unknown, fallback: string): string {
  if (!payload || typeof payload !== "object") return fallback;
  const record = payload as { error?: unknown; message?: unknown };
  if (typeof record.message === "string" && record.message.trim()) return record.message;
  if (typeof record.error === "string" && record.error.trim()) return record.error;
  return fallback;
}

function normalizeConnectorId(id: string): string {
  return MICROSOFT_ALIASES.has(id) ? "microsoft365" : id;
}

function connectorStateIds(id: string): string[] {
  if (!MICROSOFT_ALIASES.has(id)) return [id];
  return ["microsoft365", "teams", "sharepoint", "onedrive", "outlook", "m365"];
}

function groupColour(group: string): string {
  switch (group) {
    case "integration":
      return "#F5E5A8";
    case "source":
      return "#34D399";
    case "person":
    case "user":
      return "#9BD0E8";
    case "product":
      return "#F0A8A1";
    case "document":
    case "file":
    case "folder":
      return "#C7B0F0";
    case "email":
      return "#9BD0E8";
    case "channel":
    case "team":
      return "#A8E0B6";
    case "repository":
      return "#F4C16D";
    case "contact":
    case "customer":
      return "#F0A8A1";
    default:
      return "#5B5B5C";
  }
}
