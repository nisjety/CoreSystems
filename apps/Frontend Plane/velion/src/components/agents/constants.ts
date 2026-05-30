import {
  SearchIcon,
  PhoneForwardedIcon,
  MessageCircleIcon,
  CalendarIcon,
  MailIcon,
} from 'lucide-react';
import type { ComponentType, SVGProps } from 'react';

export interface AgentTool {
  id: string;
  name: string;
  description: string;
  icon: ComponentType<SVGProps<SVGSVGElement> & { className?: string; strokeWidth?: number | string }>;
}

export const AGENT_TOOLS: AgentTool[] = [
  {
    id: 'knowledge_search',
    name: 'Knowledge base search',
    description:
      'Fetches relevant excerpts from files and web pages added under Knowledge.',
    icon: SearchIcon,
  },
  {
    id: 'escalate',
    name: 'Escalate conversation',
    description:
      'Routes the customer to a human operator when AI suggests it or the customer requests it.',
    icon: PhoneForwardedIcon,
  },
  {
    id: 'mark_resolved',
    name: 'Mark as resolved',
    description: 'Ends the conversation cleanly once the issue is handled.',
    icon: MessageCircleIcon,
  },
  {
    id: 'schedule_meeting',
    name: 'Schedule meeting',
    description: 'Books meetings and demos using calendar integration.',
    icon: CalendarIcon,
  },
  {
    id: 'send_email',
    name: 'Send follow-up email',
    description: 'Sends automated follow-up emails after conversations.',
    icon: MailIcon,
  },
];
