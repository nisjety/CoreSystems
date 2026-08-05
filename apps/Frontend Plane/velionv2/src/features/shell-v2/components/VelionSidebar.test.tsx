import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VerevonSidebar } from "@/features/shell-v2/components/VerevonSidebar";

const navigationMocks = vi.hoisted(() => ({
  pathname: "/inbox",
  push: vi.fn(),
  searchParams: new URLSearchParams("view=mine"),
}));

vi.mock("next/link", () => ({
  default: ({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }) => (
    <a href={href} {...props}>
      {children}
    </a>
  ),
}));

vi.mock("next/navigation", () => ({
  usePathname: () => navigationMocks.pathname,
  useRouter: () => ({ push: navigationMocks.push }),
  useSearchParams: () => navigationMocks.searchParams,
}));

vi.mock("@/features/chat-v2/lib/chat-workspace", () => ({
  useVerevonChatWorkspaceSafe: () => null,
}));

vi.mock("@/features/chat-v2/lib/chat-format", () => ({
  formatRelative: (value: string) => value,
}));

vi.mock("@/lib/auth/auth-client", () => ({
  authClient: {
    signOut: vi.fn(),
  },
}));

function makeFetchResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

const knowledgePayload = {
  data: {
    generatedAt: "2026-06-07T08:00:00.000Z",
    orgId: "org-1",
    collections: [
      { id: "all", label: "General Knowledge", count: 3 },
      { id: "provider:notion", label: "Notion", count: 1 },
      { id: "provider:microsoft", label: "Microsoft 365", count: 1 },
      { id: "web", label: "Web sources", count: 1 },
    ],
    dataPlane: {
      available: true,
      documentCount: 3,
      indexedCount: 3,
    },
    graph: {
      available: false,
      edgeCount: 0,
      groups: [],
      nodeCount: 0,
      nodes: [],
      links: [],
      truncated: false,
    },
    metrics: { connected: 2, failed: 0, syncing: 0 },
    metricCards: [],
    folders: [
      {
        id: "source:notion",
        title: "Notion",
        subtitle: "1 retrieval document",
        providerKey: "notion",
        primaryValue: "1",
        primaryLabel: "Docs",
        secondaryValue: "1",
        secondaryLabel: "Indexed",
        connections: ["Notion", "Data Plane"],
        tone: "warm",
      },
    ],
    integrations: [],
    files: [],
    sources: [
      {
        id: "doc-shipping",
        title: "Shipping FAQ",
        description: "Notion · Indexed",
        type: "Notion",
        provider: "Notion",
        providerKey: "notion",
        category: "policy",
        owner: "System",
        updated: "5m ago",
        size: "12 KB",
        status: "Indexed",
        chunks: 3,
        hitRate: "88%",
        coverage: "100%",
        similarity: "0.88",
        tags: ["Shipping", "Policy"],
        related: ["Carrier"],
        chunksPreview: [],
      },
      {
        id: "doc-returns",
        title: "Returns Policy",
        description: "Microsoft 365 · Indexed",
        type: "Docs",
        provider: "Microsoft 365",
        providerKey: "microsoft",
        category: "policy",
        owner: "System",
        updated: "12m ago",
        size: "18 KB",
        status: "Indexed",
        chunks: 2,
        hitRate: "91%",
        coverage: "100%",
        similarity: "0.91",
        tags: ["Returns", "Policy"],
        related: ["Support"],
        chunksPreview: [],
      },
    ],
    webSources: [
      {
        id: "web-docs",
        name: "Verevon docs",
        url: "https://docs.verevon.ai",
        kind: "crawl",
        status: "active",
        providerKey: "web",
        updated: "8m ago",
      },
    ],
    finspo: {
      available: false,
      sourceCount: 0,
      largestCount: 0,
      inactiveCount: 0,
      duplicateGroups: 0,
      recommendationCount: 0,
      reclaimableBytes: 0,
    },
  },
};

function renderInboxSidebar() {
  return render(
    <VerevonSidebar
      activeRoute="/inbox"
      expanded
      onExpandedChange={vi.fn()}
      onOpenSearch={vi.fn()}
    />,
  );
}

function renderSettingsSidebar() {
  return render(
    <VerevonSidebar
      activeRoute="/settings"
      expanded
      onExpandedChange={vi.fn()}
      onOpenSearch={vi.fn()}
    />,
  );
}

function renderAccountSidebar() {
  return render(
    <VerevonSidebar
      activeRoute="/account"
      expanded
      onExpandedChange={vi.fn()}
      onOpenSearch={vi.fn()}
    />,
  );
}

function renderAgentsSidebar() {
  return render(
    <VerevonSidebar
      activeRoute="/agents"
      expanded
      onExpandedChange={vi.fn()}
      onOpenSearch={vi.fn()}
    />,
  );
}

function renderKnowledgeSidebar() {
  return render(
    <VerevonSidebar
      activeRoute="/knowledge"
      expanded
      onExpandedChange={vi.fn()}
      onOpenSearch={vi.fn()}
    />,
  );
}

describe("VerevonSidebar inbox navigation", () => {
  beforeEach(() => {
    navigationMocks.pathname = "/inbox";
    navigationMocks.searchParams = new URLSearchParams("view=mine");
    navigationMocks.push.mockReset();
  });

  it("lets inbox categories close and reopen", async () => {
    const user = userEvent.setup();
    renderInboxSidebar();

    const inboxCategory = screen.getByRole("button", { name: "Inbox" });
    expect(screen.getByRole("link", { name: /your inbox/i })).toBeVisible();

    await user.click(inboxCategory);

    expect(inboxCategory).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("link", { name: /your inbox/i })).not.toBeInTheDocument();

    await user.click(inboxCategory);

    expect(inboxCategory).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("link", { name: /your inbox/i })).toBeVisible();
  });

  it("lets sidebar rows with subitems close and reopen independently", async () => {
    const user = userEvent.setup();
    renderInboxSidebar();

    expect(screen.getByRole("link", { name: /all messages/i })).toBeVisible();

    await user.click(screen.getByRole("button", { name: /hide your inbox/i }));

    expect(screen.queryByRole("link", { name: /all messages/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /show your inbox/i }));

    expect(screen.getByRole("link", { name: /all messages/i })).toBeVisible();
    expect(screen.queryByRole("link", { name: "Facebook" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /show mentions/i }));

    expect(screen.getByRole("link", { name: "Facebook" })).toBeVisible();

    await user.click(screen.getByRole("button", { name: /hide mentions/i }));

    expect(screen.queryByRole("link", { name: "Facebook" })).not.toBeInTheDocument();
  });
});

describe("VerevonSidebar knowledge navigation", () => {
  beforeEach(() => {
    navigationMocks.pathname = "/knowledge";
    navigationMocks.searchParams = new URLSearchParams();
    navigationMocks.push.mockReset();
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(makeFetchResponse(knowledgePayload))));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("uses a dropdown selector for knowledge sidebar views", async () => {
    const user = userEvent.setup();
    renderKnowledgeSidebar();

    const selector = screen.getByRole("button", { name: /select knowledge view/i });
    expect(selector).toHaveTextContent("Knowledge Base");
    expect(screen.getByRole("navigation", { name: /knowledge navigation/i })).toBeVisible();
    expect(await screen.findByRole("button", { name: /general knowledge/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /onboarding/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /integrations/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /documents/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /rag operations/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /chunk quality/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /retrieval evals/i })).toBeVisible();

    await user.click(screen.getByRole("button", { name: /onboarding/i }));
    expect(screen.getByText(/no sources found/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: /documents/i }));

    await user.click(selector);
    await user.click(screen.getByRole("menuitemradio", { name: "Sources" }));

    expect(selector).toHaveTextContent("Sources");
    expect(screen.getByRole("button", { name: /shipping faq/i })).toBeVisible();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith(
      "/api/v1/knowledge/sources",
      expect.objectContaining({
        cache: "no-store",
        credentials: "include",
      }),
    );
  });
});

describe("VerevonSidebar agents navigation", () => {
  beforeEach(() => {
    navigationMocks.pathname = "/agents";
    navigationMocks.searchParams = new URLSearchParams();
    navigationMocks.push.mockReset();
    window.history.pushState(null, "", "/agents?agent=ecommerce&feature=commerce-shopping");
  });

  it("renders role-specific feature tabs instead of generic lifecycle tabs", () => {
    renderAgentsSidebar();

    const agentNav = screen.getByRole("navigation", { name: /agent feature tabs/i });
    expect(within(agentNav).getByRole("button", { name: /shopping assistant/i })).toBeVisible();
    expect(within(agentNav).getByRole("button", { name: /support & orders/i })).toBeVisible();
    expect(within(agentNav).getByRole("button", { name: /store actions/i })).toBeVisible();
    expect(within(agentNav).queryByRole("button", { name: /^train/i })).not.toBeInTheDocument();
    expect(within(agentNav).queryByRole("button", { name: /^deploy/i })).not.toBeInTheDocument();
  });

  it("lets keyboard users change the agent selector", async () => {
    const user = userEvent.setup();
    window.history.pushState(null, "", "/agents");

    renderAgentsSidebar();

    screen.getByRole("button", { name: /select agent type/i }).focus();
    await user.keyboard("{ArrowDown}{Enter}");

    expect(window.location.search).toContain("agent=service");
    expect(window.location.search).toContain("feature=service-resolution");
    expect(screen.getByRole("button", { name: /resolution queue/i })).toBeVisible();
  });
});

describe("VerevonSidebar settings navigation", () => {
  beforeEach(() => {
    navigationMocks.pathname = "/settings";
    navigationMocks.searchParams = new URLSearchParams();
    navigationMocks.push.mockReset();
  });

  it("renders settings section anchors inside the expanded sidebar", () => {
    renderSettingsSidebar();

    const settingsNav = screen.getByRole("navigation", { name: /settings sections/i });
    expect(within(settingsNav).getByRole("link", { name: /^workspace$/i })).toHaveAttribute("href", "/settings/workspace");
    expect(within(settingsNav).getByRole("link", { name: /^members & roles$/i })).toHaveAttribute("href", "/settings/members");
    expect(within(settingsNav).getByRole("link", { name: /^billing$/i })).toHaveAttribute("href", "/settings/billing");
    expect(within(settingsNav).getByRole("link", { name: /^sso$/i })).toHaveAttribute("href", "/settings/sso");
    expect(within(settingsNav).getByRole("link", { name: /^org security$/i })).toHaveAttribute("href", "/settings/org-security");
    expect(within(settingsNav).getByRole("link", { name: /^integrations$/i })).toHaveAttribute("href", "/settings/integrations");
    expect(within(settingsNav).queryByRole("link", { name: /^profile$/i })).not.toBeInTheDocument();
    expect(within(settingsNav).queryByRole("link", { name: /^privacy$/i })).not.toBeInTheDocument();
  });

  it("uses the same borderless row styling as other expanded sidebar links", () => {
    renderSettingsSidebar();

    const settingsNav = screen.getByRole("navigation", { name: /settings sections/i });
    const workspaceLink = within(settingsNav).getByRole("link", { name: /^workspace$/i });
    const membersLink = within(settingsNav).getByRole("link", { name: /^members & roles$/i });

    expect(workspaceLink.className).not.toContain("border");
    expect(membersLink.className).not.toContain("border");
    expect(workspaceLink).toHaveClass("rounded-[9px]");
    expect(workspaceLink).toHaveClass("bg-[#F0F1F5]");
  });
});

describe("VerevonSidebar account navigation", () => {
  beforeEach(() => {
    navigationMocks.pathname = "/account";
    navigationMocks.searchParams = new URLSearchParams();
    navigationMocks.push.mockReset();
  });

  it("links the sidebar account icon to account settings", () => {
    renderAccountSidebar();

    expect(screen.getByRole("link", { name: /^account$/i })).toHaveAttribute("href", "/account");
  });

  it("renders profile sections in the account sidebar", () => {
    renderAccountSidebar();

    const accountNav = screen.getByRole("navigation", { name: /account sections/i });
    expect(within(accountNav).getByRole("link", { name: /^profile$/i })).toHaveAttribute("href", "#profile");
    expect(within(accountNav).getByRole("link", { name: /^contact$/i })).toHaveAttribute("href", "#contact");
    expect(within(accountNav).getByRole("link", { name: /^preferences$/i })).toHaveAttribute("href", "#preferences");
    expect(within(accountNav).getByRole("link", { name: /^availability$/i })).toHaveAttribute("href", "#availability");
    expect(within(accountNav).getByRole("link", { name: /^connected accounts$/i })).toHaveAttribute("href", "#connected-accounts");
    expect(within(accountNav).getByRole("link", { name: /^privacy$/i })).toHaveAttribute("href", "#privacy");
  });
});
