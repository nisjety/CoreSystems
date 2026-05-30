export type OnboardingStep =
  | "post-signin"
  | "organization"
  | "website"
  | "connect"
  | "social-proof"
  | "paywall"
  | "assembly";

export const onboardingSteps: OnboardingStep[] = [
  "post-signin",
  "organization",
  "website",
  "connect",
  "social-proof",
  "paywall",
  "assembly",
];

export const topOnboardingSteps = onboardingSteps.slice(0, 6);

export const onboardingCopy = {
  shared: {
    supportPrefix: "Stuck?",
    back: "Back",
    backHome: "Back to home",
    switchLanguage: "Switch language",
    stepOf: "Step {{current}} of {{total}}",
    goToStep: "Go to {{step}}",
    skip: "Skip",
    stepLabels: {
      "post-signin": "Intro",
      organization: "Organization",
      website: "Website",
      connect: "Sources",
      "social-proof": "Proof",
      paywall: "Plan",
      assembly: "Setup",
    },
  },
  footer: {
    imprint: "About",
    privacy: "Privacy",
    copyright: "Copyright",
    cookieSettings: "Cookie settings",
  },
  postSignIn: {
    eyebrow: "Step 1 of 6",
    title: "All set",
    description: "We are preparing your workspace. This takes a moment.",
    spinner: "Setting up Velion …",
    overlayTitle: "Velion · for teams that answer.",
    overlayStats: "42% faster first response · 18 languages · grounded in your knowledge.",
  },
  organization: {
    eyebrow: "Step 2 of 6 · Organization",
    title: "Company name?",
    description:
      "Search the Norwegian business registry or enter the name manually. You can change everything later.",
    label: "Organization",
    sizeLegend: "How many people are you?",
    continue: "Continue",
    personalizing: "Personalizing Velion",
    enterName: "Enter the name to start",
  },
  website: {
    eyebrow: "Step 3 of 6 · Website",
    title: "Add website",
    description:
      "Paste your company URL. We will read the public pages and turn them into the agent's first knowledge base. You can add more sources right after.",
    urlLabel: "Website URL",
    briefLabel: "What should this agent help with? (optional)",
    briefPlaceholder: "Answer customer questions about products, pricing and ordering.",
    continue: "Continue",
    skip: "Skip for now",
    fetching: "Fetching content from {{url}} …",
    folderTitle: "Website knowledge",
  },
  connect: {
    eyebrow: "Step 4 of 6 · Sources",
    title: "Connect sources",
    description:
      "Pick the systems Velion should learn from. Each source becomes a node cluster in the graph on the right.",
    categories: {
      chat: "Chat",
      docs: "Documents",
      tools: "Tools",
    },
    continue: "Continue",
    skip: "Skip",
    counts: "{{sources}} sources · {{nodes}} nodes · {{edges}} edges",
  },
  socialProof: {
    eyebrow: "Step 5 of 6 · Proof",
    title: "Build with Velion",
    description:
      "We give you the same infrastructure larger support teams use, without the heavy setup. The data sources you just connected are already ready.",
    statOne: "of first requests are answered within 60 s.",
    statTwo: "faster first response after the first week.",
    statThree: "Type II · GDPR · ZDR mode available.",
    cta: "See plans",
  },
  paywall: {
    eyebrow: "Step 6 of 6 · Plan",
    title: "Smartest choice",
    why: "Why this plan",
    fullTitle: "Your best fit",
    fullSubtitle: "Select the plan that fits your needs",
    monthly: "Monthly",
    yearly: "Yearly",
    trialBadge: "14 days free",
    recommendedShort: "{{plan}} recommended",
    skipToSetup: "Skip to setup",
    continueToSetup: "Continue to setup",
    selected: "Selected",
    choosePlan: "Choose plan",
    summary:
      "Based on your onboarding signals, Advanced is a strong default: enough automation and source coverage to launch without moving into custom governance too early.",
    plans: [
      {
        id: "trial",
        name: "Free",
        description: "Try Velion and the agent for 14 days before choosing a paid plan.",
        price: "$0",
        cadence: "/mo",
        badge: "Free",
        features: ["No card required", "14 day trial", "Upgrade when you are ready"],
      },
      {
        id: "hobby",
        name: "Essential",
        description: "For small teams validating a simple chatbot.",
        price: "$25",
        yearlyPrice: "$20",
        cadence: "/mo",
        features: ["AI resolved conversations", "Chatbot + shared inbox", "Website knowledge sources"],
      },
      {
        id: "standard",
        name: "Advanced",
        description: "For teams that need automation, routing and more sources.",
        price: "$99",
        yearlyPrice: "$85",
        cadence: "/mo",
        features: ["Per conversation resolved by AI", "Automation and routing", "Multiple team inboxes"],
      },
      {
        id: "pro",
        name: "Expert",
        description: "For larger support teams with reporting and control needs.",
        price: "$149",
        yearlyPrice: "$110",
        cadence: "/mo",
        features: ["SSO and identity governance", "SLA reporting", "Multibrand controls"],
      },
      {
        id: "enterprise",
        name: "Custom",
        description: "For volume, onboarding and governance requirements.",
        price: "Sales",
        cadence: "",
        features: ["Volume pricing", "Custom terms", "Dedicated success team"],
      },
    ],
  },
  assembly: {
    eyebrow: "Done",
    title: "Preparing Velion",
    description:
      "We are moving in everything we collected: knowledge, integrations and your agent, then opening the dashboard in a few seconds.",
    ticks: [
      "Setting up the workspace",
      "Importing knowledge from the website",
      "Connecting integrations",
      "Training the first agent",
      "Preparing the dashboard",
    ],
  },
} as const;

export function formatOnboardingText(template: string, values: Record<string, string | number>) {
  return Object.entries(values).reduce(
    (out, [key, value]) =>
      out.replace(new RegExp(`{{\\s*${key}\\s*}}`, "g"), String(value)),
    template,
  );
}
