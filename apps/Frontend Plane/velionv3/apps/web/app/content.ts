export const siteConfig = {
  name: "Velion",
  navItems: ["Product", "Trust", "Workflows", "Demo"],
  primaryCta: "Book a demo",
  secondaryCta: "See how Velion works",
  heroTitle: "The AI teammate for customer experience",
  heroBody:
    "Velion answers customers, learns your company, proposes the right work, and executes approved actions across support, sales, knowledge, and workflows.",
} as const;

export const proofPoints = [
  {
    title: "AI that understands your business",
    body: "Company knowledge, source traces, and channel context stay connected.",
  },
  {
    title: "Human approvals built in",
    body: "Risky replies and system actions wait for a clear human yes.",
  },
  {
    title: "Acts across systems with guardrails",
    body: "Every autonomous path has a manual path, audit trail, and rollback boundary.",
  },
] as const;

export const conversations = [
  {
    channel: "Kunde via e-post",
    subject: "Hvor er bestillingen min?",
    time: "09:41",
    state: "New",
  },
  {
    channel: "Live chat - Ida",
    subject: "Kan jeg bytte størrelse?",
    time: "09:32",
    state: "Open",
  },
  {
    channel: "Instagram - @nordic.outdoor",
    subject: "Er jakken vanntett?",
    time: "09:18",
    state: "Draft",
  },
  {
    channel: "Facebook - Mia",
    subject: "Retur og refusjon",
    time: "08:45",
    state: "Open",
  },
] as const;

export const sourceTrace = [
  {
    title: "Order confirmation #89213",
    path: "checkout/order/89213",
    status: "Cited",
  },
  {
    title: "Shipping policy",
    path: "docs/shipping.md",
    status: "Cited",
  },
  {
    title: "Delivery times - Norway",
    path: "docs/delivery-no.md",
    status: "Cited",
  },
] as const;

export const actionTimeline = [
  "Received customer email",
  "Matched order and policy sources",
  "Drafted answer in brand tone",
  "Waiting for approval",
] as const;

export const workflows = [
  {
    number: "01",
    title: "Resolve support",
    body: "Velion understands the issue, finds answers, and drafts a response in your tone.",
    steps: ["Message in", "AI understands", "Draft response", "Resolve"],
  },
  {
    number: "02",
    title: "Ground knowledge",
    body: "Connect sources once. Velion learns your content and cites what it uses.",
    steps: ["Connect sources", "Index and sync", "AI answers", "Cited result"],
  },
  {
    number: "03",
    title: "Approve actions",
    body: "Let Velion take action across systems within policies and approvals you set.",
    steps: ["Propose action", "Review and approve", "Execute safely", "Audit log"],
  },
] as const;

export const trustPillars = [
  {
    title: "Human in control",
    body: "Velion proposes. You approve. Always.",
  },
  {
    title: "Audit every action",
    body: "Every action is logged with who, what, when, and why.",
  },
  {
    title: "Rollback ready",
    body: "Revert or undo actions with built-in safety boundaries.",
  },
  {
    title: "EU data residency",
    body: "Run customer work in EU and EEA regions you choose.",
  },
  {
    title: "Enterprise secure",
    body: "Role-based access controls, encryption boundaries, and least-privilege actions.",
  },
] as const;

export const footerColumns = [
  {
    title: "Product",
    links: ["Overview", "Workflows", "Integrations", "Pricing"],
  },
  {
    title: "Trust",
    links: ["Security", "Data residency", "Compliance", "Audit"],
  },
  {
    title: "Company",
    links: ["About us", "Careers", "Partners", "Contact"],
  },
  {
    title: "Resources",
    links: ["Docs", "Blog", "Events"],
  },
] as const;
