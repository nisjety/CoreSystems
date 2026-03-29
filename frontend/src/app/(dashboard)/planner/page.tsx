import type { Metadata } from 'next';
import { PlannerWorkspaceClient } from '@/components/planner/PlannerWorkspaceClient';

export const metadata: Metadata = {
  title: 'Planner',
  description: 'Write, draw and plan in your own frontend, with BlockSuite on the canvas.',
};

export default function PlannerPage() {
  return <PlannerWorkspaceClient />;
}
