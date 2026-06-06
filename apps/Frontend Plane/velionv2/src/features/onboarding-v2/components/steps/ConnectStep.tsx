"use client";

/**
 * Step 4 — knowledge connectors with live graph reveal.
 *
 * Left: grouped source picker (Chat / Docs / Tools). Clicking a row creates a
 * connect session via the BFF (org + user resolved server-side from the
 * session), starts Velion direct OAuth from the Velion-styled row, and records
 * the pick only after auth succeeds.
 *
 * Right: deterministic-radial node-link graph driven by graph-index-rs, polled
 * via React Query (dedup + cancellation handled by the query client).
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Minus, Plus, RotateCcw, X } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  ConnectUnavailableError,
  cleanupOnboardingSource,
  createConnectSession,
  discoverConnectorSource,
  disconnectConnections,
  fetchIntegrationSummary,
  fetchGraphPreview,
  startIntegrationSync,
  warmSharePointDiscovery,
  type ConnectSessionResult,
  type PreviewEdge,
  type PreviewNode,
  type PreviewResponse,
} from "../../lib/onboarding-api";
import { buildSafeConnectorMetadata } from "../../lib/onboarding-evidence";
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

const GRAPH_POLL_INTERVAL_MS = 2_500;
const EMPTY_NODES: PreviewNode[] = [];
const EMPTY_EDGES: PreviewEdge[] = [];
const MICROSOFT_ALIASES = new Set(["teams", "sharepoint", "onedrive", "outlook", "m365", "microsoft365", "microsoft-365"]);
const PROVIDER_AUTH_POPUP_WIDTH = 540;
const PROVIDER_AUTH_POPUP_HEIGHT = 720;
const PROVIDER_AUTH_TIMEOUT_MS = 2 * 60 * 1000;
const GRAPH_VIEW_BOX_SIZE = 420;
const GRAPH_MIN_ZOOM = 0.65;
const GRAPH_MAX_ZOOM = 3.2;

export function ConnectStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy } = useOnboardingCopy();
  const queryClient = useQueryClient();
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(machine.state.connectors.map((c) => normalizeConnectorId(c.id))),
  );
  const [connectError, setConnectError] = useState<string | null>(null);
  const [connectUnavailable, setConnectUnavailable] = useState(false);
  const [connectingId, setConnectingId] = useState<string | null>(null);

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
      const pendingMetadata = buildSafeConnectorMetadata(
        { id: connector.id, label: connector.label },
        { status: "pending", scopes: connector.sources },
      );
      machine.addConnector({
        id: connector.id,
        label: connector.label,
        authedAt: new Date().toISOString(),
        metadata: pendingMetadata,
      });
      setPicked((prev) => (prev.has(connector.id) ? prev : new Set([...prev, connector.id])));
      setConnectingId(null);
      setConnectError(null);
      if (connector.id === "microsoft365") {
        void warmSharePointDiscovery(machine.state.organization?.id);
      }
      void discoverConnectorSource({
        provider: connector.provider,
        connectorId: connector.id,
        label: connector.label,
        sources: connector.sources,
        orgId: machine.state.organization?.id,
      }).then((result) => {
        machine.updateConnectorMetadata(connector.id, {
          ...result.metadata,
          status: result.graphSeedStatus === "failed" ? "failed" : result.metadata.status,
          seedDocumentId: result.graphSeedDocumentId ?? result.metadata.seedDocumentId,
        });
        nudgeGraph();
      });
      void startConnectorSync(connector, machine, nudgeGraph);
      nudgeGraph();
    },
    [machine, nudgeGraph],
  );

  const toggle = useCallback(
    async (connector: ConnectorMeta) => {
      if (picked.has(connector.id)) {
        const matchingIds = connectorStateIds(connector.id);
        const existing = machine.state.connectors.find((item) => matchingIds.includes(item.id));
        machine.updateConnectorMetadata(connector.id, {
          status: "removed",
          workspaceName: existing?.metadata?.workspaceName ?? connector.label,
          sensitivity: "safe_metadata_only",
          discoveredAt: new Date().toISOString(),
          cleanupStatus: "requested",
          seedDocumentId: existing?.metadata?.seedDocumentId,
        });
        void Promise.allSettled([
          cleanupOnboardingSource({
            connectorId: connector.id,
            documentId: existing?.metadata?.seedDocumentId,
            source: `onboarding:${connector.provider}:${connector.id}`,
          }),
          disconnectConnections([connector.id]),
        ]).finally(nudgeGraph);
        matchingIds.forEach((id) => machine.removeConnector(id));
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
      setConnectUnavailable(false);
      const authWindow = openProviderAuthWindow();
      let authWindowNavigated = false;
      try {
        const session = await createConnectSession({ provider: connector.provider, sources: connector.sources });
        authWindowNavigated = true;
        await startProviderAuth(session, authWindow);
        completeConnector(connector);
      } catch (error) {
        if (!authWindowNavigated) closeProviderAuthWindow(authWindow);
        if (error instanceof ConnectUnavailableError) {
          setConnectUnavailable(true);
        } else {
          setConnectError(connectAuthError(error, connector.label, copy.connect.couldNotConnect, locale));
        }
        setConnectingId(null);
      }
    },
    [completeConnector, copy.connect.couldNotConnect, locale, machine, nudgeGraph, picked],
  );

  const submit = () => machine.goTo("social-proof");

  const sourceCount =
    picked.size +
    (connectingId && !picked.has(normalizeConnectorId(connectingId)) ? 1 : 0) +
    (machine.state.website?.url ? 1 : 0);
  const previewGraph = useMemo(
    () =>
      withOnboardingSources(
        graph ?? null,
        machine.state.website,
        machine.state.organization,
        picked,
        connectingId,
      ),
    [connectingId, graph, machine.state.organization, machine.state.website, picked],
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

        {connectUnavailable ? (
          <div className="rounded-md border border-[#E5DFD3] bg-[#F7F4ED] px-3.5 py-3" role="status">
            <p className="font-inter text-[12px] font-medium leading-5 text-[#1F1B17]">
              {connectUnavailableText(locale).title}
            </p>
            <p className="mt-1 font-inter text-[11px] leading-4 text-[#6B6660]">
              {connectUnavailableText(locale).body}
            </p>
          </div>
        ) : connectError ? (
          <p className="font-inter text-[12px] leading-5 text-[#B42318]" role="alert">
            {connectError}
          </p>
        ) : null}

        <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 pt-2">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <PrimaryButton onClick={submit}>{copy.connect.continue}</PrimaryButton>
            <SkipLink onClick={submit}>{copy.connect.skip}</SkipLink>
          </div>
          <p className="whitespace-nowrap font-mono text-[10px] uppercase tracking-[0.14em] text-[#A09890]">
            {formatOnboardingText(copy.connect.counts, {
              sources: sourceCount,
              nodes: counts.nodes,
              edges: counts.edges,
            })}
          </p>
        </div>
      </LeftPane>

      <RightPane showIcons={false} showScanner={false}>
        <GraphReveal
          graph={previewGraph}
          warning={previewGraph?.warning ?? null}
          sourceCount={sourceCount}
          countsTemplate={copy.connect.graphCounts}
          locale={locale}
        />
      </RightPane>
    </>
  );
}

function connectUnavailableText(locale: OnboardingLocale): { title: string; body: string } {
  return locale === "nb"
    ? {
        title: "Kildekobling er ikke tilgjengelig i dette miljøet ennå.",
        body: "Du kan hoppe over nå og koble til kilder senere fra innstillinger.",
      }
    : {
        title: "Connector setup isn't available in this environment yet.",
        body: "You can skip for now and connect sources later from settings.",
      };
}

async function startProviderAuth(
  session: ConnectSessionResult,
  authWindow: Window | null,
): Promise<void> {
  if (!session.sessionToken || !session.connectUrl) {
    throw new Error("Integration service returned an incomplete connect session.");
  }
  if (session.authMode !== "direct-oauth") {
    throw new Error("Integration service returned a legacy connect session. Direct OAuth is required.");
  }
  await runDirectOauthWindow({ connectUrl: session.connectUrl, sessionToken: session.sessionToken, authWindow });
}

function openProviderAuthWindow(): Window | null {
  const layout = centeredPopupLayout({
    expectedWidth: PROVIDER_AUTH_POPUP_WIDTH,
    expectedHeight: PROVIDER_AUTH_POPUP_HEIGHT,
  });
  return window.open("", "_blank", popupFeatures(layout));
}

function closeProviderAuthWindow(authWindow: Window | null): void {
  if (!authWindow || authWindow.closed) return;
  try {
    authWindow.close();
  } catch {
    // Best-effort cleanup for the pre-opened blank popup.
  }
}

function centeredPopupLayout({
  expectedWidth,
  expectedHeight,
}: {
  expectedWidth: number;
  expectedHeight: number;
}): { left: number; top: number; width: number; height: number } {
  const screenWidth = window.screen.width;
  const screenHeight = window.screen.height;
  const width = Math.min(expectedWidth, screenWidth);
  const height = Math.min(expectedHeight, screenHeight);
  return {
    left: Math.max(screenWidth / 2 - width / 2, 0),
    top: Math.max(screenHeight / 2 - height / 2, 0),
    width,
    height,
  };
}

function popupFeatures(layout: { left: number; top: number; width: number; height: number }): string {
  return [
    `left=${layout.left}`,
    `top=${layout.top}`,
    `width=${layout.width}`,
    `height=${layout.height}`,
    "scrollbars=yes",
    "resizable=yes",
    "status=no",
    "toolbar=no",
    "location=no",
    "copyhistory=no",
    "menubar=no",
    "directories=no",
  ].join(",");
}

function runDirectOauthWindow({
  connectUrl,
  sessionToken,
  authWindow,
}: {
  connectUrl: string;
  sessionToken?: string;
  authWindow: Window | null;
}): Promise<void> {
  if (!authWindow) {
    return Promise.reject(new ProviderAuthFlowError("blocked_by_browser", "Modal blocked by browser"));
  }

  return new Promise<void>((resolve, reject) => {
    let settled = false;
    const settle = (fn: () => void) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timeout);
      window.clearInterval(closePoll);
      window.removeEventListener("message", handleMessage);
      fn();
    };

    const handleMessage = (event: MessageEvent) => {
      const payload = event.data;
      if (!payload || typeof payload !== "object") return;
      const record = payload as Record<string, unknown>;
      if (record.type !== "velion.integration.connected") return;
      if (sessionToken && record.sessionToken !== sessionToken) return;
      if (record.status === "success") {
        settle(() => resolve());
        return;
      }
      const message =
        typeof record.message === "string" && record.message.trim()
          ? record.message
          : "The authorization flow did not complete.";
      settle(() =>
        reject(
          new ProviderAuthFlowError(
            typeof record.errorCode === "string" ? record.errorCode : "authorization_failed",
            message,
          ),
        ),
      );
    };

    window.addEventListener("message", handleMessage);

    const timeout = window.setTimeout(() => {
      settle(() =>
        reject(
          new ProviderAuthFlowError(
            "timeout",
            "The authorization flow timed out before the provider returned a result.",
          ),
        ),
      );
    }, PROVIDER_AUTH_TIMEOUT_MS);

    const closePoll = window.setInterval(() => {
      if (!authWindow.closed) return;
      settle(() =>
        reject(
          new ProviderAuthFlowError(
            "window_closed",
            "The authorization window was closed before the connection finished.",
          ),
        ),
      );
    }, 500);

    try {
      authWindow.location.href = connectUrl;
    } catch {
      settle(() =>
        reject(
          new ProviderAuthFlowError(
            "window_closed",
            "The authorization window was closed before the connection finished.",
          ),
        ),
      );
    }
  });
}

class ProviderAuthFlowError extends Error {
  constructor(
    readonly type: string,
    message: string,
  ) {
    super(message);
    this.name = "ProviderAuthFlowError";
  }
}

function connectAuthError(
  error: unknown,
  label: string,
  fallbackTemplate: string,
  locale: OnboardingLocale,
): string {
  if (error instanceof ProviderAuthFlowError) {
    if (error.type === "blocked_by_browser") {
      return locale === "nb"
        ? `Nettleseren blokkerte innloggingen for ${label}. Tillat popups for Velion og prøv igjen.`
        : `Pop-up was blocked while connecting ${label}. Allow pop-ups for Velion and try again.`;
    }
    if (error.type === "window_closed") {
      return locale === "nb"
        ? `${label}-innloggingen ble lukket før koblingen var ferdig.`
        : `The ${label} sign-in window was closed before the connection finished.`;
    }
    return error.message || formatOnboardingText(fallbackTemplate, { label });
  }
  return error instanceof Error
    ? error.message
    : formatOnboardingText(fallbackTemplate, { label });
}

async function startConnectorSync(
  connector: ConnectorMeta,
  machine: OnboardingMachine,
  nudgeGraph: () => void,
): Promise<void> {
  const connection = await waitForProviderConnection(connector.provider);
  if (!connection?.id) return;
  const result = await startIntegrationSync(connection.id);
  if (!result.syncJob?.status) return;
  applyConnectorSyncStatus(connector, machine, result.syncJob.status);
  if (result.syncJob.id) {
    watchConnectorSyncProgress(result.syncJob.id, connector, machine, nudgeGraph);
  }
  nudgeGraph();
}

function applyConnectorSyncStatus(
  connector: ConnectorMeta,
  machine: OnboardingMachine,
  status: string,
): void {
  const existing = machine.state.connectors.find((item) => item.id === connector.id)?.metadata;
  machine.updateConnectorMetadata(connector.id, {
    ...existing,
    status: status === "failed" ? "failed" : "pending",
    workspaceName: existing?.workspaceName ?? connector.label,
    sensitivity: "safe_metadata_only",
    discoveredAt: existing?.discoveredAt ?? new Date().toISOString(),
    entityCounts: { ...(existing?.entityCounts ?? {}), sync_jobs: 1 },
    sampleEntities: [...(existing?.sampleEntities ?? []).slice(0, 3), `Sync ${status}`],
    scopes: existing?.scopes ?? connector.sources,
  });
}

function watchConnectorSyncProgress(
  jobId: string,
  connector: ConnectorMeta,
  machine: OnboardingMachine,
  nudgeGraph: () => void,
): void {
  if (typeof window === "undefined" || typeof EventSource === "undefined") return;
  const source = new EventSource(`/api/v1/integrations/sync-jobs/${encodeURIComponent(jobId)}/events`);
  const closeTimer = window.setTimeout(() => source.close(), 30_000);
  const handleEvent = (event: MessageEvent<string>) => {
    const status = syncStatusFromEventData(event.data);
    if (!status) return;
    applyConnectorSyncStatus(connector, machine, status);
    nudgeGraph();
    if (["completed", "failed", "cancelled", "handoff_data_plane", "waiting_provider"].includes(status)) {
      window.clearTimeout(closeTimer);
      source.close();
    }
  };
  for (const eventName of [
    "sync.queued",
    "sync.running",
    "sync.waiting_provider",
    "sync.handoff_data_plane",
    "sync.completed",
    "sync.failed",
    "sync.cancelled",
    "sync.snapshot",
  ]) {
    source.addEventListener(eventName, handleEvent);
  }
  source.onerror = () => {
    window.clearTimeout(closeTimer);
    source.close();
  };
}

function syncStatusFromEventData(data: string): string | null {
  try {
    const payload = JSON.parse(data) as {
      type?: unknown;
      status?: unknown;
      syncJob?: { status?: unknown };
    };
    const status = typeof payload.status === "string"
      ? payload.status
      : typeof payload.syncJob?.status === "string"
        ? payload.syncJob.status
        : null;
    if (status) return status;
    return typeof payload.type === "string" && payload.type.startsWith("sync.")
      ? payload.type.replace(/^sync\./, "")
      : null;
  } catch {
    return null;
  }
}

async function waitForProviderConnection(provider: string): Promise<{ id: string } | null> {
  const target = normalizeProviderKey(provider);
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const summary = await fetchIntegrationSummary();
    const connection = summary.connections.find((item) => normalizeProviderKey(item.providerKey) === target);
    if (connection?.id) return connection;
    await delay(700);
  }
  return null;
}

function normalizeProviderKey(provider: string): string {
  const key = provider.trim().toLowerCase();
  if (["gdrive", "google-drive", "google-workspace", "gmail"].includes(key)) return "google";
  if (["m365", "microsoft365", "microsoft-365", "microsoft-graph"].includes(key)) return "microsoft";
  return key;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
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

/* ----------------------------------------------------------- graph render */

function GraphReveal({
  graph,
  warning,
  sourceCount,
  countsTemplate,
  locale,
}: {
  graph: PreviewResponse | null;
  warning: string | null;
  sourceCount: number;
  countsTemplate: string;
  locale: OnboardingLocale;
}) {
  const graphRef = useRef<SVGSVGElement | null>(null);
  const previousIdsRef = useRef<Set<string>>(new Set());
  const dragRef = useRef<GraphDragState | null>(null);
  const [highlightedIds, setHighlightedIds] = useState<Set<string>>(new Set());
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [view, setView] = useState<GraphViewState>({ x: 0, y: 0, zoom: 1 });
  const [dragging, setDragging] = useState(false);
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
  const edges = displayGraph?.edges ?? EMPTY_EDGES;
  const showDenseLabels = nodes.length <= 24;
  const activeSelectedNodeId = useMemo(() => {
    if (!selectedNodeId) return null;
    return nodes.some((node) => node.id === selectedNodeId) ? selectedNodeId : null;
  }, [nodes, selectedNodeId]);
  const selectedNode = useMemo(
    () => nodes.find((node) => node.id === activeSelectedNodeId) ?? null,
    [activeSelectedNodeId, nodes],
  );
  const selectedConnections = useMemo(
    () => (selectedNode ? connectedNodesFor(selectedNode.id, nodes, edges) : []),
    [edges, nodes, selectedNode],
  );

  const changeZoom = useCallback((delta: number) => {
    setView((current) => zoomGraphView(current, delta));
  }, []);

  useEffect(() => {
    const graphEl = graphRef.current;
    if (!graphEl) return;
    const handleWheel = (event: globalThis.WheelEvent) => {
      event.preventDefault();
      changeZoom(event.deltaY > 0 ? -0.18 : 0.18);
    };
    graphEl.addEventListener("wheel", handleWheel, { passive: false });
    return () => graphEl.removeEventListener("wheel", handleWheel);
  }, [changeZoom]);

  const resetView = useCallback(() => {
    setView({ x: 0, y: 0, zoom: 1 });
  }, []);

  const handlePointerDown = useCallback(
    (event: PointerEvent<SVGSVGElement>) => {
      if (event.target instanceof Element && event.target.closest("[data-graph-node='true']")) return;
      const rect = event.currentTarget.getBoundingClientRect();
      dragRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        originX: view.x,
        originY: view.y,
        width: rect.width,
        height: rect.height,
      };
      event.currentTarget.setPointerCapture(event.pointerId);
      setDragging(true);
    },
    [view.x, view.y],
  );

  const handlePointerMove = useCallback((event: PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const dx = ((event.clientX - drag.startX) / drag.width) * GRAPH_VIEW_BOX_SIZE;
    const dy = ((event.clientY - drag.startY) / drag.height) * GRAPH_VIEW_BOX_SIZE;
    setView((current) => ({
      ...current,
      x: drag.originX + dx,
      y: drag.originY + dy,
    }));
  }, []);

  const endDrag = useCallback((event: PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(false);
  }, []);

  return (
    <div className="relative flex size-full items-center justify-center overflow-hidden bg-[#0F0F10] p-6">
      <div
        aria-hidden="true"
        className="absolute inset-0 opacity-[0.08]"
        style={{
          backgroundImage:
            "linear-gradient(rgba(255,255,255,0.45) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.45) 1px, transparent 1px)",
          backgroundSize: "36px 36px",
        }}
      />
      <svg
        ref={graphRef}
        viewBox={`0 0 ${GRAPH_VIEW_BOX_SIZE} ${GRAPH_VIEW_BOX_SIZE}`}
        className={cn("relative h-[90%] w-[90%] touch-none", dragging ? "cursor-grabbing" : "cursor-grab")}
        role="img"
        aria-label={locale === "nb" ? "Interaktiv kildegraf" : "Interactive source graph"}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <g transform={`translate(${view.x} ${view.y}) scale(${view.zoom})`}>
          {edges.map((edge, i) => {
            const a = layout.get(edge.a);
            const b = layout.get(edge.b);
            const selectedEdge = activeSelectedNodeId === edge.a || activeSelectedNodeId === edge.b;
            if (!a || !b) return null;
            return (
              <line
                key={`${edge.a}-${edge.b}-${i}`}
                x1={a.cx}
                y1={a.cy}
                x2={b.cx}
                y2={b.cy}
                stroke={selectedEdge ? "#E5DFD3" : "#4A4A4D"}
                strokeWidth={selectedEdge ? 1.7 : 1}
                strokeOpacity={selectedEdge ? 0.9 : 0.78}
              />
            );
          })}
          {nodes.map((node) => {
            const pos = layout.get(node.id);
            if (!pos) return null;
            const isOrg = node.group === "org";
            const selected = activeSelectedNodeId === node.id;
            const showLabel = showDenseLabels || isOrg || node.group === "integration" || node.group === "source";
            const justArrived = highlightedIds.has(node.id);
            const fill = isOrg ? "#F5E5A8" : justArrived ? "#34D399" : groupColour(node.group);
            return (
              <g key={node.id}>
                <circle
                  data-graph-node="true"
                  role="button"
                  tabIndex={0}
                  aria-label={`${node.label}, ${graphGroupLabel(node.group, locale)}`}
                  className="cursor-pointer outline-none"
                  cx={pos.cx}
                  cy={pos.cy}
                  r={14}
                  fill="transparent"
                  stroke={selected ? "#E5DFD3" : "#34D39955"}
                  strokeWidth={selected ? 1.5 : 5}
                  opacity={selected || justArrived ? 1 : 0}
                  onPointerDown={(event) => event.stopPropagation()}
                  onClick={(event) => {
                    event.stopPropagation();
                    setSelectedNodeId(node.id);
                  }}
                  onKeyDown={(event: KeyboardEvent<SVGCircleElement>) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    event.preventDefault();
                    setSelectedNodeId(node.id);
                  }}
                />
                <circle
                  cx={pos.cx}
                  cy={pos.cy}
                  r={isOrg ? 8 : node.group === "integration" ? 6.5 : justArrived ? 5.6 : 4.4}
                  fill={fill}
                  stroke={selected ? "#0F0F10" : "transparent"}
                  strokeWidth={selected ? 1.5 : 0}
                  pointerEvents="none"
                />
                {showLabel && (
                  <text
                    x={pos.cx}
                    y={pos.cy + 22}
                    textAnchor="middle"
                    className="pointer-events-none select-none font-mono"
                    fontSize={isOrg ? 11 : node.group === "integration" ? 10 : 9}
                    fill={selected ? "#FFFFFF" : isOrg ? "#E5DFD3" : "#C9C2B9"}
                  >
                    {truncateNodeLabel(node.label)}
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>
      <GraphControls
        zoom={view.zoom}
        locale={locale}
        onZoomIn={() => changeZoom(0.22)}
        onZoomOut={() => changeZoom(-0.22)}
        onReset={resetView}
      />
      <div className="pointer-events-none absolute left-6 top-6 flex w-[min(290px,calc(100%-140px))] flex-col gap-2">
        {warning && (
          <div className="rounded-lg border border-white/10 bg-black/45 px-3 py-2 backdrop-blur">
            <p className="font-inter text-[10px] leading-4 text-white/70">{warning}</p>
          </div>
        )}
        <GraphInspector
          node={selectedNode}
          connections={selectedConnections}
          locale={locale}
          onClose={() => setSelectedNodeId(null)}
        />
      </div>
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

interface GraphViewState {
  x: number;
  y: number;
  zoom: number;
}

interface GraphDragState {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  width: number;
  height: number;
}

function GraphControls({
  zoom,
  locale,
  onZoomIn,
  onZoomOut,
  onReset,
}: {
  zoom: number;
  locale: OnboardingLocale;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onReset: () => void;
}) {
  const copy = graphControlCopy(locale);
  return (
    <div className="absolute right-6 top-6 flex items-center gap-1.5 rounded-lg border border-white/10 bg-black/45 p-1.5 backdrop-blur">
      <GraphIconButton label={copy.zoomOut} onClick={onZoomOut}>
        <Minus className="size-3.5" strokeWidth={2} />
      </GraphIconButton>
      <span className="min-w-11 text-center font-mono text-[10px] tabular-nums text-white/70">
        {Math.round(zoom * 100)}%
      </span>
      <GraphIconButton label={copy.zoomIn} onClick={onZoomIn}>
        <Plus className="size-3.5" strokeWidth={2} />
      </GraphIconButton>
      <GraphIconButton label={copy.reset} onClick={onReset}>
        <RotateCcw className="size-3.5" strokeWidth={2} />
      </GraphIconButton>
    </div>
  );
}

function GraphIconButton({
  children,
  label,
  onClick,
}: {
  children: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-md border border-white/10 bg-white/[0.06] text-white/80 transition-colors hover:bg-white/[0.12] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white/60"
    >
      {children}
    </button>
  );
}

function GraphInspector({
  node,
  connections,
  locale,
  onClose,
}: {
  node: PreviewNode | null;
  connections: PreviewNode[];
  locale: OnboardingLocale;
  onClose: () => void;
}) {
  if (!node) return null;
  const copy = graphInspectorCopy(locale);
  const connectionLabel = connections.length === 1 ? copy.connection : copy.connections;
  return (
    <aside className="pointer-events-auto rounded-lg border border-white/10 bg-black/55 p-3 shadow-[0_18px_60px_rgba(0,0,0,0.26)] backdrop-blur" aria-live="polite">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="font-mono text-[9px] uppercase tracking-[0.18em] text-white/45">{copy.eyebrow}</p>
          <h3 className="mt-1 truncate font-inter text-[14px] font-semibold leading-5 text-white">{node.label}</h3>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label={copy.close}
          title={copy.close}
          className="grid size-7 shrink-0 place-items-center rounded-md border border-white/10 bg-white/[0.06] text-white/70 transition-colors hover:bg-white/[0.12]"
        >
          <X className="size-3.5" strokeWidth={2} />
        </button>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-2">
        <GraphDetailStat label={copy.type} value={graphGroupLabel(node.group, locale)} />
        <GraphDetailStat label={copy.links} value={`${connections.length} ${connectionLabel}`} />
      </div>

      <div className="mt-3 rounded-md border border-white/10 bg-white/[0.04] px-2.5 py-2">
        <p className="font-mono text-[9px] uppercase tracking-[0.16em] text-white/45">{copy.data}</p>
        <p className="mt-1 line-clamp-2 font-inter text-[11px] leading-4 text-white/72">
          {graphNodeSummary(node, connections.length, locale)}
        </p>
        <p className="mt-2 truncate font-mono text-[9px] text-white/38">{node.id}</p>
      </div>

      {connections.length > 0 && (
        <div className="mt-3">
          <p className="font-mono text-[9px] uppercase tracking-[0.16em] text-white/45">{copy.connectedTo}</p>
          <div className="mt-1.5 flex flex-col gap-1">
            {connections.slice(0, 5).map((connection) => (
              <div key={connection.id} className="flex min-w-0 items-center gap-2 rounded-md bg-white/[0.04] px-2 py-1.5">
                <span
                  aria-hidden="true"
                  className="size-1.5 shrink-0 rounded-full"
                  style={{ backgroundColor: groupColour(connection.group) }}
                />
                <span className="truncate font-inter text-[10.5px] text-white/72">{connection.label}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      <p className="mt-3 border-t border-white/10 pt-3 font-inter text-[10.5px] leading-4 text-white/48">
        {copy.knowledgeHint}
      </p>
    </aside>
  );
}

function GraphDetailStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md border border-white/10 bg-white/[0.04] px-2.5 py-2">
      <p className="font-mono text-[9px] uppercase tracking-[0.16em] text-white/38">{label}</p>
      <p className="mt-1 truncate font-inter text-[11px] font-medium text-white/78">{value}</p>
    </div>
  );
}

function graphControlCopy(locale: OnboardingLocale): { zoomIn: string; zoomOut: string; reset: string } {
  return locale === "nb"
    ? { zoomIn: "Zoom inn", zoomOut: "Zoom ut", reset: "Tilbakestill visning" }
    : { zoomIn: "Zoom in", zoomOut: "Zoom out", reset: "Reset view" };
}

function graphInspectorCopy(locale: OnboardingLocale): {
  eyebrow: string;
  close: string;
  type: string;
  links: string;
  connection: string;
  connections: string;
  data: string;
  connectedTo: string;
  knowledgeHint: string;
} {
  return locale === "nb"
    ? {
        eyebrow: "Node · kun innsyn",
        close: "Lukk node",
        type: "Type",
        links: "Koblinger",
        connection: "kobling",
        connections: "koblinger",
        data: "Data",
        connectedTo: "Koblet til",
        knowledgeHint: "Grafen kan inspiseres her. Redigering, fjerning og audit skjer senere i Knowledge.",
      }
    : {
        eyebrow: "Node · read only",
        close: "Close node",
        type: "Type",
        links: "Links",
        connection: "link",
        connections: "links",
        data: "Data",
        connectedTo: "Connected to",
        knowledgeHint: "Inspect the graph here. Editing, removal and audit happen later in Knowledge.",
      };
}

function graphGroupLabel(group: string, locale: OnboardingLocale): string {
  const nb: Record<string, string> = {
    org: "Organisasjon",
    integration: "Kilde",
    source: "Nettside",
    channel: "Kanal",
    team: "Team",
    email: "E-post",
    document: "Dokument",
    file: "Fil",
    folder: "Mappe",
    repository: "Repo",
    person: "Person",
    user: "Bruker",
    product: "Produkt",
    contact: "Kontakt",
    customer: "Kunde",
  };
  const en: Record<string, string> = {
    org: "Organization",
    integration: "Source",
    source: "Website",
    channel: "Channel",
    team: "Team",
    email: "Email",
    document: "Document",
    file: "File",
    folder: "Folder",
    repository: "Repository",
    person: "Person",
    user: "User",
    product: "Product",
    contact: "Contact",
    customer: "Customer",
  };
  const labels = locale === "nb" ? nb : en;
  return labels[group] ?? group.replace(/[_-]/g, " ");
}

function graphNodeSummary(node: PreviewNode, connectionCount: number, locale: OnboardingLocale): string {
  if (locale === "nb") {
    if (node.group === "org") return `Arbeidsområdet samler ${connectionCount} kilder og dataområder.`;
    if (node.group === "integration") return `${node.label} er koblet inn som en kilde med ${connectionCount} dataområder.`;
    if (node.id.startsWith("website:")) return `${node.label} kommer fra nettside-crawlen i forrige steg.`;
    return `${node.label} er et ${graphGroupLabel(node.group, locale).toLowerCase()}-område i grafen.`;
  }
  if (node.group === "org") return `The workspace anchor connects ${connectionCount} sources and data areas.`;
  if (node.group === "integration") return `${node.label} is connected as a source with ${connectionCount} data areas.`;
  if (node.id.startsWith("website:")) return `${node.label} comes from the website crawl in the previous step.`;
  return `${node.label} is a ${graphGroupLabel(node.group, locale).toLowerCase()} area in the graph.`;
}

function connectedNodesFor(nodeId: string, nodes: PreviewNode[], edges: PreviewEdge[]): PreviewNode[] {
  const nodesById = new Map(nodes.map((node) => [node.id, node]));
  const connectedIds = new Set<string>();
  for (const edge of edges) {
    if (edge.a === nodeId) connectedIds.add(edge.b);
    if (edge.b === nodeId) connectedIds.add(edge.a);
  }
  return Array.from(connectedIds)
    .map((id) => nodesById.get(id))
    .filter((node): node is PreviewNode => Boolean(node));
}

function zoomGraphView(view: GraphViewState, delta: number): GraphViewState {
  const nextZoom = clamp(view.zoom + delta, GRAPH_MIN_ZOOM, GRAPH_MAX_ZOOM);
  const center = GRAPH_VIEW_BOX_SIZE / 2;
  const graphX = (center - view.x) / view.zoom;
  const graphY = (center - view.y) / view.zoom;
  return {
    zoom: nextZoom,
    x: center - graphX * nextZoom,
    y: center - graphY * nextZoom,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

interface NodePos {
  cx: number;
  cy: number;
}

function layoutNodes(nodes: PreviewNode[]): Map<string, NodePos> {
  const out = new Map<string, NodePos>();
  const center = { cx: 210, cy: 210 };
  const org = nodes.find((node) => node.group === "org");
  if (org) out.set(org.id, center);

  const integrations = nodes.filter((node) => node.group === "integration");
  const angles =
    integrations.length === 1
      ? [-Math.PI / 2]
      : integrations.length === 2
        ? [-Math.PI * 0.78, -Math.PI * 0.22]
        : integrations.map((_, index) => -Math.PI * 0.92 + ((Math.PI * 1.84) / (integrations.length - 1)) * index);
  integrations.forEach((node, index) => {
    const angle = angles[index] ?? -Math.PI / 2;
    out.set(node.id, {
      cx: center.cx + Math.cos(angle) * 130,
      cy: center.cy + Math.sin(angle) * 118,
    });
  });

  const childrenByParent = new Map<string, PreviewNode[]>();
  const looseNodes: PreviewNode[] = [];
  for (const node of nodes) {
    if (node.group === "org" || node.group === "integration") continue;
    const parentId = parentConnectorNodeId(node.id);
    if (parentId && out.has(parentId)) {
      childrenByParent.set(parentId, [...(childrenByParent.get(parentId) ?? []), node]);
    } else {
      looseNodes.push(node);
    }
  }

  for (const [parentId, children] of childrenByParent.entries()) {
    const parent = out.get(parentId);
    if (!parent) continue;
    const parentAngle = Math.atan2(parent.cy - center.cy, parent.cx - center.cx);
    const spread = Math.min(Math.PI * 1.25, 0.5 * Math.max(1, children.length - 1));
    children.forEach((node, index) => {
      const childAngle = parentAngle - spread / 2 + (children.length === 1 ? 0 : (spread / (children.length - 1)) * index);
      out.set(node.id, {
        cx: parent.cx + Math.cos(childAngle) * 68,
        cy: parent.cy + Math.sin(childAngle) * 58,
      });
    });
  }

  looseNodes.forEach((node, index) => {
    if (out.has(node.id)) return;
    if (node.id.startsWith("website:")) {
      out.set(node.id, { cx: 210, cy: 342 });
      return;
    }
    const seed = hashId(node.id);
    const angle = ((seed % 240) + 150 + index * 17) * (Math.PI / 180);
    const radius = 140 + ((seed >> 8) % 30);
    out.set(node.id, { cx: center.cx + Math.cos(angle) * radius, cy: center.cy + Math.sin(angle) * radius });
  });

  for (const node of nodes) {
    if (out.has(node.id)) continue;
    const seed = hashId(node.id);
    const angle = (seed % 360) * (Math.PI / 180);
    out.set(node.id, { cx: center.cx + Math.cos(angle) * 150, cy: center.cy + Math.sin(angle) * 132 });
  }
  return out;
}

function parentConnectorNodeId(nodeId: string): string | null {
  const match = /^connector:([^:]+):/.exec(nodeId);
  return match ? `connector:${match[1]}` : null;
}

function hashId(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) hash = (hash * 31 + input.charCodeAt(i)) | 0;
  return Math.abs(hash);
}

function truncateNodeLabel(label: string): string {
  return label.length > 18 ? `${label.slice(0, 17)}...` : label;
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
    const key = `${edge.a}\0${edge.b}`;
    if (edgesByKey.has(key)) continue;
    edgesByKey.set(key, edge);
  }
  const nodes = Array.from(nodesById.values());
  const edges = Array.from(edgesByKey.values());
  const groups = new Set(nodes.filter((n) => n.group !== "org").map((n) => n.group));
  return { nodes, edges, counts: { nodes: nodes.length, edges: edges.length, groups: groups.size }, warning: graph.warning };
}

function withOnboardingSources(
  graph: PreviewResponse | null,
  website?: WebsitePayload,
  organization?: OrganizationPayload,
  connectorIds?: Set<string>,
  connectingId?: string | null,
): PreviewResponse | null {
  const url = website?.url?.trim();
  const nodes = [...(graph?.nodes ?? [])];
  const edges = [...(graph?.edges ?? [])];
  const nodeIds = new Set(nodes.map((n) => n.id));
  let orgNodeId = nodes.find((n) => n.group === "org")?.id;
  if (!orgNodeId) {
    orgNodeId = "org";
    nodes.push({ id: orgNodeId, label: organization?.id?.slice(0, 8) || organization?.name || "Org", group: "org" });
    nodeIds.add(orgNodeId);
  }

  if (url) {
    const websiteNodeId = `website:${safeSourceIdPart(url)}`;
    if (!nodeIds.has(websiteNodeId)) {
      nodes.push({ id: websiteNodeId, label: websiteHost(url), group: "source" });
      nodeIds.add(websiteNodeId);
    }
    addEdgeOnce(edges, orgNodeId, websiteNodeId);
  }

  const activeIds = new Set(Array.from(connectorIds ?? []).map(normalizeConnectorId));
  if (connectingId) activeIds.add(normalizeConnectorId(connectingId));
  for (const id of activeIds) {
    const connector = CONNECTORS.find((candidate) => candidate.id === id);
    if (!connector) continue;
    const connectorNodeId = `connector:${safeSourceIdPart(connector.id)}`;
    if (!nodeIds.has(connectorNodeId)) {
      nodes.push({ id: connectorNodeId, label: connector.label, group: "integration" });
      nodeIds.add(connectorNodeId);
    }
    addEdgeOnce(edges, orgNodeId, connectorNodeId);
    for (const source of connector.sources) {
      const sourceNodeId = `${connectorNodeId}:${safeSourceIdPart(source)}`;
      if (!nodeIds.has(sourceNodeId)) {
        nodes.push({ id: sourceNodeId, label: sourceLabel(source), group: sourceGroup(source) });
        nodeIds.add(sourceNodeId);
      }
      addEdgeOnce(edges, connectorNodeId, sourceNodeId);
    }
  }

  const groups = new Set(nodes.filter((n) => n.group !== "org").map((n) => n.group));
  return normalisePreviewGraph({ nodes, edges, counts: { nodes: nodes.length, edges: edges.length, groups: groups.size }, warning: graph?.warning });
}

function addEdgeOnce(edges: PreviewEdge[], a: string, b: string): void {
  if (!edges.some((edge) => edge.a === a && edge.b === b)) edges.push({ a, b });
}

function sourceLabel(source: string): string {
  const labels: Record<string, string> = {
    messages: "Messages",
    teams: "Teams",
    outlook: "Outlook",
    sharepoint: "SharePoint",
    onedrive: "OneDrive",
    pages: "Pages",
    databases: "Databases",
    google_drive: "Drive",
    documents: "Documents",
    issues: "Issues",
  };
  return labels[source] ?? source.replace(/[_-]/g, " ");
}

function sourceGroup(source: string): string {
  switch (source) {
    case "messages":
    case "teams":
      return "channel";
    case "outlook":
      return "email";
    case "sharepoint":
    case "onedrive":
    case "google_drive":
    case "documents":
    case "pages":
    case "databases":
      return "document";
    case "issues":
      return "repository";
    default:
      return "source";
  }
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
