import { useReducer } from 'react'

interface SidebarState {
  query: string
  editingDocId: string | null
  draftTitle: string
  collapsedSections: string[]
  collapsedNodes: string[]
  draggedDocumentId: string | null
  dropTargetId: string | null
}

type SidebarAction =
  | { type: 'SET_QUERY'; payload: string }
  | { type: 'START_EDITING'; payload: { docId: string; title: string } }
  | { type: 'SET_DRAFT_TITLE'; payload: string }
  | { type: 'STOP_EDITING' }
  | { type: 'TOGGLE_SECTION'; payload: string }
  | { type: 'TOGGLE_NODE'; payload: string }
  | { type: 'DRAG_START'; payload: string }
  | { type: 'SET_DROP_TARGET'; payload: string | null }
  | { type: 'CLEAR_DRAG' }

const initialState: SidebarState = {
  query: '',
  editingDocId: null,
  draftTitle: '',
  collapsedSections: ['trash'],
  collapsedNodes: [],
  draggedDocumentId: null,
  dropTargetId: null,
}

function reducer(state: SidebarState, action: SidebarAction): SidebarState {
  switch (action.type) {
    case 'SET_QUERY':
      return { ...state, query: action.payload }
    case 'START_EDITING':
      return { ...state, editingDocId: action.payload.docId, draftTitle: action.payload.title }
    case 'SET_DRAFT_TITLE':
      return { ...state, draftTitle: action.payload }
    case 'STOP_EDITING':
      return { ...state, editingDocId: null, draftTitle: '' }
    case 'TOGGLE_SECTION':
      return {
        ...state,
        collapsedSections: state.collapsedSections.includes(action.payload)
          ? state.collapsedSections.filter((v) => v !== action.payload)
          : [...state.collapsedSections, action.payload],
      }
    case 'TOGGLE_NODE':
      return {
        ...state,
        collapsedNodes: state.collapsedNodes.includes(action.payload)
          ? state.collapsedNodes.filter((v) => v !== action.payload)
          : [...state.collapsedNodes, action.payload],
      }
    case 'DRAG_START':
      return { ...state, draggedDocumentId: action.payload, dropTargetId: null }
    case 'SET_DROP_TARGET':
      return { ...state, dropTargetId: action.payload }
    case 'CLEAR_DRAG':
      return { ...state, draggedDocumentId: null, dropTargetId: null }
    default:
      return state
  }
}

export function usePlannerSidebarState() {
  const [state, dispatch] = useReducer(reducer, initialState)
  return { state, dispatch } as const
}

export type { SidebarState, SidebarAction }
