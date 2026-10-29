import type { ChatbotAddOnId, ChatbotBuilderSectionId } from '@/features/agents/lib/agent-roles'

type StudioSection = {
  id: ChatbotBuilderSectionId
  title: string
  description: string
  status: string
}

export const chatbotDisplayName = 'Verevon Support Agent'

export const studioSections: Record<ChatbotBuilderSectionId, StudioSection> = {
  playground: {
    id: 'playground',
    title: 'Playground',
    description: 'Tune model, instructions, tools, message behavior, and the live chatbot preview.',
    status: 'Runtime checked live',
  },
  'data-sources': {
    id: 'data-sources',
    title: 'Fine-tuning',
    description: 'Prepare training files, run real model fine-tuning jobs, and manage adapters.',
    status: 'No datasets uploaded',
  },
  integrations: {
    id: 'integrations',
    title: 'Integrations',
    description: 'Connect the systems the chatbot can fetch data from before tools use them.',
    status: 'Live status checked',
  },
  actions: {
    id: 'actions',
    title: 'Tools',
    description: 'Configure the tools, skill pool, and executable capabilities the chatbot can use.',
    status: 'Guarded actions',
  },
  'chat-logs': {
    id: 'chat-logs',
    title: 'Chat logs',
    description: 'Review conversations and inspect the live playground transcript.',
    status: 'No conversations yet',
  },
  analytics: {
    id: 'analytics',
    title: 'Analytics',
    description: 'Track chat count, topic trends, and sentiment across conversations.',
    status: 'Waiting for live events',
  },
  leads: {
    id: 'leads',
    title: 'Leads',
    description: 'Review lead submissions collected by the chatbot.',
    status: 'No leads captured',
  },
  insights: {
    id: 'insights',
    title: 'Insights',
    description: 'Summarize performance, countries, feedback, and improvement opportunities.',
    status: 'Waiting for signals',
  },
  install: {
    id: 'install',
    title: 'Install',
    description: 'Install the chatbot across web widgets, help pages, email, and messaging channels.',
    status: 'Configure channels',
  },
  settings: {
    id: 'settings',
    title: 'Settings',
    description: 'Control chatbot identity, tone, safety, and defaults.',
    status: 'Draft changes',
  },
}

export const defaultVisibleAddOns: ChatbotAddOnId[] = ['subscription-action', 'instructions', 'guidance']
export const allAddOnIds: ChatbotAddOnId[] = [
  'subscription-action',
  'instructions',
  'channels',
  'user-info',
  'guidance',
  'billing-analytics',
  'automation-rate',
  'performance',
]
