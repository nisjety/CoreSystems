import type {
  Agent,
  CalendarEvent,
  CalendarNote,
  CustomerContext,
  Group,
  Macro,
  TicketSentiment,
  ZammadArticle,
  ZammadTicket,
} from '@/features/inbox/lib/inbox-model'

export const demoAgents: Agent[] = [
  { id: 11, firstname: 'Ava', lastname: 'Lunde', email: 'ava@velion.local' },
  { id: 12, firstname: 'Jonas', lastname: 'Hagen', email: 'jonas@velion.local' },
  { id: 13, firstname: 'Mina', lastname: 'Berg', email: 'mina@velion.local' },
]

export const demoGroups: Group[] = [
  { id: 1, name: 'Support' },
  { id: 2, name: 'Admin Support' },
  { id: 3, name: 'Billing' },
]

export const demoTickets: ZammadTicket[] = [
  {
    id: 42,
    number: '54172',
    title: 'Order marked delivered but missing',
    state: { id: 2, name: 'open' },
    priority: { id: 3, name: 'high' },
    group: demoGroups[0],
    owner: demoAgents[0],
    customer: { id: 7, firstname: 'Maya', lastname: 'Solberg', email: 'maya@example.com' },
    tags: ['delivery', 'urgent', '@support'],
    created_at: '2026-06-09T09:35:00.000Z',
    updated_at: '2026-06-12T08:58:00.000Z',
    article_count: 4,
    channel: 'email',
    agentState: 'routed',
  },
  {
    id: 43,
    number: '54183',
    title: 'Refund window for annual plan',
    state: { id: 6, name: 'pending reminder' },
    priority: { id: 2, name: 'normal' },
    group: demoGroups[2],
    owner: demoAgents[1],
    customer: { id: 8, firstname: 'Elias', lastname: 'Nilsen', email: 'elias@example.com' },
    tags: ['refund', 'billing'],
    created_at: '2026-06-08T12:15:00.000Z',
    updated_at: '2026-06-11T16:26:00.000Z',
    article_count: 3,
    channel: 'messenger',
    agentState: 'all',
  },
  {
    id: 44,
    number: '54204',
    title: 'WhatsApp handoff for VIP onboarding',
    state: { id: 2, name: 'open' },
    priority: { id: 3, name: 'high' },
    group: demoGroups[1],
    owner: null,
    customer: { id: 9, firstname: 'Nora', lastname: 'Bakke', email: 'nora@example.com' },
    tags: ['vip', 'onboarding', 'mention'],
    created_at: '2026-06-10T07:40:00.000Z',
    updated_at: '2026-06-12T07:14:00.000Z',
    article_count: 5,
    channel: 'whatsapp',
    agentState: 'abandoned',
  },
  {
    id: 45,
    number: '54219',
    title: 'Invoice attachment fails to upload',
    state: { id: 2, name: 'open' },
    priority: { id: 2, name: 'normal' },
    group: demoGroups[0],
    owner: demoAgents[2],
    customer: { id: 10, firstname: 'Oskar', lastname: 'Lien', email: 'oskar@example.com' },
    tags: ['invoice', 'attachment'],
    created_at: '2026-06-11T10:04:00.000Z',
    updated_at: '2026-06-12T06:51:00.000Z',
    article_count: 2,
    channel: 'email',
    agentState: 'all',
  },
  {
    id: 46,
    number: '54220',
    title: 'Facebook mention about delayed shipment',
    state: { id: 2, name: 'open' },
    priority: { id: 1, name: 'low' },
    group: demoGroups[0],
    owner: null,
    customer: { id: 11, firstname: 'Sara', lastname: 'Moen', email: 'sara@example.com' },
    tags: ['social', 'mention'],
    created_at: '2026-06-11T13:24:00.000Z',
    updated_at: '2026-06-11T19:08:00.000Z',
    article_count: 1,
    channel: 'facebook',
    agentState: 'all',
  },
  {
    id: 47,
    number: '54221',
    title: 'Resolved chatbot exchange needs QA',
    state: { id: 4, name: 'closed' },
    priority: { id: 2, name: 'normal' },
    group: demoGroups[0],
    owner: demoAgents[0],
    customer: { id: 12, firstname: 'Henrik', lastname: 'Dahl', email: 'henrik@example.com' },
    tags: ['ai-resolved'],
    created_at: '2026-06-07T08:11:00.000Z',
    updated_at: '2026-06-11T11:12:00.000Z',
    article_count: 6,
    channel: 'messenger',
    agentState: 'resolved',
  },
]

export const demoArticlesByTicketId: Record<number, ZammadArticle[]> = {
  42: [
    {
      id: 1001,
      ticket_id: 42,
      body: 'My package says it was delivered yesterday, but nothing arrived. The carrier page has not changed since the morning.',
      sender: 'Customer',
      from: 'Maya Solberg',
      internal: false,
      created_at: '2026-06-12T07:46:00.000Z',
    },
    {
      id: 1002,
      ticket_id: 42,
      body: 'Carrier scan is delayed. Check shipment exception policy before promising a replacement.',
      sender: 'Agent',
      from: 'Ava Lunde',
      internal: true,
      created_at: '2026-06-12T08:02:00.000Z',
    },
    {
      id: 1003,
      ticket_id: 42,
      body: 'Hi Maya, I found the carrier event and I am checking the delivery proof with the warehouse team now.',
      sender: 'Agent',
      from: 'Velion Support',
      internal: false,
      created_at: '2026-06-12T08:17:00.000Z',
    },
  ],
  43: [
    {
      id: 1101,
      ticket_id: 43,
      body: 'Can you confirm whether the annual plan can be refunded if we cancel before the 14-day mark?',
      sender: 'Customer',
      from: 'Elias Nilsen',
      internal: false,
      created_at: '2026-06-11T15:03:00.000Z',
    },
    {
      id: 1102,
      ticket_id: 43,
      body: 'Yes, the annual subscription can be refunded inside the trial window. I can start that review for you.',
      sender: 'Agent',
      from: 'Jonas Hagen',
      internal: false,
      created_at: '2026-06-11T16:26:00.000Z',
    },
  ],
  44: [
    {
      id: 1201,
      ticket_id: 44,
      body: 'We need to move the onboarding conversation to a human. The bot keeps asking the same two questions.',
      sender: 'Customer',
      from: 'Nora Bakke',
      internal: false,
      created_at: '2026-06-12T06:52:00.000Z',
    },
    {
      id: 1202,
      ticket_id: 44,
      body: 'VIP account. Route to Admin Support and summarize the failed bot loop before replying.',
      sender: 'Agent',
      from: 'Velion AI',
      internal: true,
      created_at: '2026-06-12T07:14:00.000Z',
    },
  ],
}

export const demoSentimentsByTicketId: Record<number, TicketSentiment> = {
  42: { sentiment: 'negative', score: 0.78 },
  43: { sentiment: 'neutral', score: 0.56 },
  44: { sentiment: 'frustrated', score: 0.91 },
  45: { sentiment: 'neutral', score: 0.49 },
  46: { sentiment: 'negative', score: 0.62 },
  47: { sentiment: 'positive', score: 0.71 },
}

export const demoQuickReplies = [
  'Hi, thanks for flagging this. I am checking the latest account and delivery context now.',
  'I found the relevant policy and will keep the next step inside this conversation.',
  'I can route this to the right team and add a private note with the current context.',
]

export const demoMacros: Macro[] = [
  { id: 1, name: 'Generic: Sign Off' },
  { id: 2, name: 'Refund window explanation' },
  { id: 3, name: 'Shipping update request' },
]

export const demoCustomerContext: CustomerContext = {
  shopify: {
    orders: [
      {
        id: 'ord_1842',
        name: '#1842',
        fulfillment_status: 'In transit',
        created_at: '2026-06-08',
        total_price: '129.00',
      },
      {
        id: 'ord_1771',
        name: '#1771',
        fulfillment_status: 'Fulfilled',
        created_at: '2026-05-21',
        total_price: '64.00',
      },
    ],
  },
  stripe: {
    customer: { id: 'cus_8MAYA', email: 'maya@example.com' },
    subscription: { status: 'active', plan: 'Pro yearly' },
  },
}

export const demoCalendarEvents: CalendarEvent[] = [
  {
    id: 'cal_followup_1',
    title: 'Follow up: missing package',
    start: '2026-06-12T09:00:00.000Z',
    end: '2026-06-12T09:30:00.000Z',
    type: 'inbox-follow-up',
    status: 'confirmed',
    createdAt: '2026-06-11T16:22:00.000Z',
  },
  {
    id: 'cal_review_1',
    title: 'QA review: resolved chatbot exchange',
    start: '2026-06-13T11:00:00.000Z',
    end: '2026-06-13T11:30:00.000Z',
    type: 'inbox-ai-review',
    status: 'confirmed',
    createdAt: '2026-06-11T17:15:00.000Z',
  },
]

export const demoCalendarNotes: CalendarNote[] = [
  {
    id: 'note_1',
    text: '#54172: Check carrier scan before promising replacement.',
    date: '2026-06-12',
    createdAt: '2026-06-11T18:00:00.000Z',
  },
]
