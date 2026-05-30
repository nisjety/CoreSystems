import { useReducer } from 'react'

interface User {
  id: string
  email: string
  name: string
  emailVerified: boolean
  createdAt: string
  lastLoginAt?: string
  isActive: boolean
  isBlocked: boolean
  isSuspended: boolean
}

interface UserStatsData {
  totalUsers: number
  activeUsers: number
  totalOrganizations: number
  totalApiKeys: number
  activeSessions: number
  recentSignUps: number
  recentLogins: number
}

interface AdminUserState {
  users: User[]
  stats: UserStatsData | null
  loading: boolean
  error: string | null
  showCreateModal: boolean
  showEditModal: boolean
  showDeleteModal: boolean
  selectedUser: User | null
  currentPage: number
  totalPages: number
  searchQuery: string
}

type AdminUserAction =
  | { type: 'SET_USERS'; payload: User[] }
  | { type: 'SET_STATS'; payload: UserStatsData | null }
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_TOTAL_PAGES'; payload: number }
  | { type: 'SHOW_CREATE_MODAL' }
  | { type: 'SHOW_EDIT_MODAL'; payload: User }
  | { type: 'SHOW_DELETE_MODAL'; payload: User }
  | { type: 'CLOSE_MODALS' }
  | { type: 'SET_PAGE'; payload: number }
  | { type: 'SET_SEARCH'; payload: string }
  | { type: 'FETCH_SUCCESS'; payload: { users: User[]; totalPages: number } }

const initialState: AdminUserState = {
  users: [],
  stats: null,
  loading: true,
  error: null,
  showCreateModal: false,
  showEditModal: false,
  showDeleteModal: false,
  selectedUser: null,
  currentPage: 1,
  totalPages: 1,
  searchQuery: '',
}

function reducer(state: AdminUserState, action: AdminUserAction): AdminUserState {
  switch (action.type) {
    case 'SET_USERS':
      return { ...state, users: action.payload }
    case 'SET_STATS':
      return { ...state, stats: action.payload }
    case 'SET_LOADING':
      return { ...state, loading: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_TOTAL_PAGES':
      return { ...state, totalPages: action.payload }
    case 'SHOW_CREATE_MODAL':
      return { ...state, showCreateModal: true }
    case 'SHOW_EDIT_MODAL':
      return { ...state, showEditModal: true, selectedUser: action.payload }
    case 'SHOW_DELETE_MODAL':
      return { ...state, showDeleteModal: true, selectedUser: action.payload }
    case 'CLOSE_MODALS':
      return { ...state, showCreateModal: false, showEditModal: false, showDeleteModal: false, selectedUser: null }
    case 'SET_PAGE':
      return { ...state, currentPage: action.payload }
    case 'SET_SEARCH':
      return { ...state, searchQuery: action.payload, currentPage: 1 }
    case 'FETCH_SUCCESS':
      return { ...state, users: action.payload.users, totalPages: action.payload.totalPages, loading: false }
    default:
      return state
  }
}

export function useAdminUserState() {
  return useReducer(reducer, initialState)
}

export type { AdminUserState, AdminUserAction }
