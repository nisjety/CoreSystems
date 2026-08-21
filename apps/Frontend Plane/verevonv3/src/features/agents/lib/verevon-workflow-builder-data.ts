import {
  CalendarClock,
  Database,
  Split,
  type LucideProps,
} from '@/shared/icons'
import type { Component } from 'solid-js'
import type { WorkflowBuilderToolId } from '@/features/agents/lib/agent-roles'

export type CanvasNodeId =
  | 'schedule-daily-post'
  | 'generate-image'
  | 'generate-caption'
  | 'post-facebook'
  | 'post-instagram'
  | 'post-linkedin'
  | 'merge'
  | 'update-status'

export type BrandMarkId =
  | 'openai'
  | 'facebook'
  | 'instagram'
  | 'linkedin'
  | 'sheets'
  | 'gemini'
  | 'grok'
  | 'perplexity'
  | 'drive'
  | 'slides'
  | 'docs'
  | 'slack'
  | 'notion'

type WorkflowIcon = Component<LucideProps>

export type WorkflowNode = {
  id: CanvasNodeId
  title: string
  Icon?: WorkflowIcon
  brand?: BrandMarkId
  x: number
  y: number
  tall?: boolean
}

export type InspectorField = {
  label: string
  value: string
}

export type ToolInspector = {
  title: string
  Icon?: WorkflowIcon
  brand?: BrandMarkId
  fields: InspectorField[]
  prompt: string
  outputs: string[]
  nextLabel: string
}

export const toolToCanvasNode: Record<WorkflowBuilderToolId, CanvasNodeId> = {
  'schedule-daily-post': 'schedule-daily-post',
  'generate-image': 'generate-image',
  'generate-caption': 'generate-caption',
  'post-facebook': 'post-facebook',
  'post-instagram': 'post-instagram',
  'post-linkedin': 'post-linkedin',
  merge: 'merge',
  'update-status': 'update-status',
  chatgpt: 'generate-caption',
  gemini: 'generate-caption',
  grok: 'generate-caption',
  perplexity: 'generate-caption',
  'google-drive': 'update-status',
  'google-slides': 'update-status',
  'google-docs': 'update-status',
  slack: 'merge',
  notion: 'update-status',
}

export const workflowNodes: WorkflowNode[] = [
  { id: 'schedule-daily-post', title: 'Schedule\nDaily Post', Icon: CalendarClock, x: 72, y: 304 },
  { id: 'generate-image', title: 'Generate Image', brand: 'openai', x: 240, y: 304 },
  { id: 'generate-caption', title: 'Generate Caption', brand: 'openai', x: 410, y: 304 },
  { id: 'post-facebook', title: 'Post on\nFacebook', brand: 'facebook', x: 590, y: 178 },
  { id: 'post-instagram', title: 'Post on\nInstagram', brand: 'instagram', x: 590, y: 304 },
  { id: 'post-linkedin', title: 'Post on\nLinkedIn', brand: 'linkedin', x: 590, y: 430 },
  { id: 'merge', title: 'Merge', Icon: Split, x: 760, y: 304, tall: true },
  { id: 'update-status', title: 'Update Status\nto DONE', brand: 'sheets', x: 930, y: 304 },
]

export const inspectorByTool: Record<CanvasNodeId, ToolInspector> = {
  'schedule-daily-post': {
    title: 'Schedule Daily Post',
    Icon: CalendarClock,
    fields: [
      { label: 'Trigger', value: 'Schedule' },
      { label: 'Frequency', value: 'Daily' },
      { label: 'Timezone', value: 'Workspace' },
    ],
    prompt: 'Start the workflow every weekday at 09:00 and pass the campaign brief to the AI generation steps.',
    outputs: ['Run date', 'Campaign brief'],
    nextLabel: 'Next',
  },
  'generate-image': {
    title: 'Generate Image',
    brand: 'openai',
    fields: [
      { label: 'Provider', value: 'Open AI' },
      { label: 'Authentication', value: 'Open AI' },
      { label: 'Model', value: 'Image generation' },
    ],
    prompt: 'Create one clean social image that matches the campaign brief, leaves room for caption context, and avoids text-heavy layouts.',
    outputs: ['Image URL', 'Alt text'],
    nextLabel: 'Next',
  },
  'generate-caption': {
    title: 'Generate Caption',
    Icon: Database,
    fields: [
      { label: 'Provider', value: 'Open AI' },
      { label: 'Authentication', value: 'Open AI' },
      { label: 'Model', value: 'Open AI' },
    ],
    prompt: 'Create an engaging Instagram post caption that includes:\n- A compelling hook to grab attention\n- Relevant hashtags (5-10 popular ones)\n- A call-to-action that encourages engagement\n- A compelling hook to grab attention\n- Relevant hashtags (5-10 popular ones)',
    outputs: ['Caption', 'Hashtags'],
    nextLabel: 'Next',
  },
  'post-facebook': {
    title: 'Post on Facebook',
    brand: 'facebook',
    fields: [
      { label: 'Provider', value: 'Facebook' },
      { label: 'Authentication', value: 'Meta OAuth' },
      { label: 'Page', value: 'Team project' },
    ],
    prompt: 'Publish the approved caption and image to Facebook with the campaign tracking fields attached.',
    outputs: ['Post URL', 'Published at'],
    nextLabel: 'Save',
  },
  'post-instagram': {
    title: 'Post on Instagram',
    brand: 'instagram',
    fields: [
      { label: 'Provider', value: 'Instagram' },
      { label: 'Authentication', value: 'Meta OAuth' },
      { label: 'Account', value: 'Team project' },
    ],
    prompt: 'Post the generated image and caption to Instagram, preserve the hashtag block, and return the published media URL.',
    outputs: ['Media URL', 'Post status'],
    nextLabel: 'Save',
  },
  'post-linkedin': {
    title: 'Post on LinkedIn',
    brand: 'linkedin',
    fields: [
      { label: 'Provider', value: 'LinkedIn' },
      { label: 'Authentication', value: 'LinkedIn OAuth' },
      { label: 'Profile', value: 'Company page' },
    ],
    prompt: 'Adapt the caption into a concise LinkedIn post, keep the image, and return the post URL for the merge step.',
    outputs: ['Post URL', 'Audience'],
    nextLabel: 'Save',
  },
  merge: {
    title: 'Merge',
    Icon: Split,
    fields: [
      { label: 'Mode', value: 'Wait for all' },
      { label: 'Inputs', value: '3 branches' },
      { label: 'Failure path', value: 'Continue' },
    ],
    prompt: 'Collect the Facebook, Instagram, and LinkedIn results into one payload for the final status update.',
    outputs: ['Branch results', 'Summary'],
    nextLabel: 'Next',
  },
  'update-status': {
    title: 'Update Status',
    brand: 'sheets',
    fields: [
      { label: 'Provider', value: 'Google Sheets' },
      { label: 'Authentication', value: 'Workspace OAuth' },
      { label: 'Sheet', value: 'Content calendar' },
    ],
    prompt: 'Update the row for this campaign to DONE and attach the published URLs from each social channel.',
    outputs: ['Row ID', 'Status'],
    nextLabel: 'Publish',
  },
}

export const workflowToolBrandMap: Partial<Record<WorkflowBuilderToolId, BrandMarkId>> = {
  chatgpt: 'openai',
  gemini: 'gemini',
  grok: 'grok',
  perplexity: 'perplexity',
  'google-drive': 'drive',
  'update-status': 'sheets',
  'google-slides': 'slides',
  'google-docs': 'docs',
  slack: 'slack',
  notion: 'notion',
  'post-linkedin': 'linkedin',
  'post-instagram': 'instagram',
  'post-facebook': 'facebook',
}

export const workflowToolIconMap: Partial<Record<WorkflowBuilderToolId, WorkflowIcon>> = {
  'schedule-daily-post': CalendarClock,
  merge: Split,
}
