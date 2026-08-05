import { createContext, useContext, type Accessor } from 'solid-js'
import type { WorkspaceIdentity } from '@/features/core/lib/shell-data'

export const CoreWorkspaceContext = createContext<Accessor<WorkspaceIdentity>>()

export function useCoreWorkspace(): Accessor<WorkspaceIdentity | undefined> {
  return useContext(CoreWorkspaceContext) ?? (() => undefined)
}
