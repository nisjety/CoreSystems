import { useReducer } from 'react';
import type { AcItem } from './AutocompleteDropdown';

// ── Types ─────────────────────────────────────────────────────────────────

export interface Attachment {
  id: string;
  url: string;
  name: string;
}

export interface EntityToken {
  start: number;
  text: string;
  kind: 'date' | 'file' | 'person';
}

export interface AcState {
  items: AcItem[];
  category: string;
  triggerStart: number;
  triggerLen: number;
}

export type ResponseMode = 'auto' | 'quick' | 'deep';

// ── State shape ───────────────────────────────────────────────────────────

export interface ChatInputState {
  localModel: string;
  deepSearch: boolean;
  browseWeb: boolean;
  responseMode: ResponseMode;
  modeAnnouncement: string | null;
  showSuggestions: boolean;
  isRecording: boolean;
  attachments: Attachment[];
  acState: AcState | null;
  acIndex: number;
  entities: EntityToken[];
}

// ── Actions ───────────────────────────────────────────────────────────────

type Action =
  | { type: 'SET_MODEL'; model: string }
  | { type: 'SET_DEEP_SEARCH'; on: boolean }
  | { type: 'SET_BROWSE_WEB'; on: boolean }
  | { type: 'SET_RESPONSE_MODE'; mode: ResponseMode; announcement: string }
  | { type: 'CLEAR_ANNOUNCEMENT' }
  | { type: 'TOGGLE_SUGGESTIONS' }
  | { type: 'HIDE_SUGGESTIONS' }
  | { type: 'SET_RECORDING'; on: boolean }
  | { type: 'ADD_ATTACHMENTS'; items: Attachment[] }
  | { type: 'REMOVE_ATTACHMENT'; id: string }
  | { type: 'CLEAR_ATTACHMENTS' }
  | { type: 'SET_AUTOCOMPLETE'; ac: AcState | null }
  | { type: 'SET_AC_INDEX'; index: number }
  | { type: 'SET_ENTITIES'; entities: EntityToken[] }
  | { type: 'ADD_ENTITY'; entity: EntityToken }
  | { type: 'PRUNE_ENTITIES'; message: string };

// ── Reducer ───────────────────────────────────────────────────────────────

function reducer(state: ChatInputState, action: Action): ChatInputState {
  switch (action.type) {
    case 'SET_MODEL':
      return { ...state, localModel: action.model };

    case 'SET_DEEP_SEARCH':
      return { ...state, deepSearch: action.on };

    case 'SET_BROWSE_WEB':
      return { ...state, browseWeb: action.on };

    case 'SET_RESPONSE_MODE':
      return {
        ...state,
        responseMode: action.mode,
        modeAnnouncement: action.announcement,
      };

    case 'CLEAR_ANNOUNCEMENT':
      return { ...state, modeAnnouncement: null };

    case 'TOGGLE_SUGGESTIONS':
      return { ...state, showSuggestions: !state.showSuggestions };

    case 'HIDE_SUGGESTIONS':
      return { ...state, showSuggestions: false };

    case 'SET_RECORDING':
      return { ...state, isRecording: action.on };

    case 'ADD_ATTACHMENTS':
      return { ...state, attachments: [...state.attachments, ...action.items] };

    case 'REMOVE_ATTACHMENT': {
      const att = state.attachments.find(a => a.id === action.id);
      if (att) URL.revokeObjectURL(att.url);
      return { ...state, attachments: state.attachments.filter(a => a.id !== action.id) };
    }

    case 'CLEAR_ATTACHMENTS': {
      state.attachments.forEach(a => URL.revokeObjectURL(a.url));
      return { ...state, attachments: [] };
    }

    case 'SET_AUTOCOMPLETE':
      return { ...state, acState: action.ac, acIndex: action.ac ? 0 : state.acIndex };

    case 'SET_AC_INDEX':
      return { ...state, acIndex: action.index };

    case 'SET_ENTITIES':
      return { ...state, entities: action.entities };

    case 'ADD_ENTITY':
      return { ...state, entities: [...state.entities, action.entity] };

    case 'PRUNE_ENTITIES':
      return {
        ...state,
        entities: state.entities.filter(ent => {
          const end = ent.start + ent.text.length;
          return end <= action.message.length && action.message.slice(ent.start, end) === ent.text;
        }),
      };

    default:
      return state;
  }
}

// ── Hook ──────────────────────────────────────────────────────────────────

export function useChatInputState(initialModel: string) {
  return useReducer(reducer, {
    localModel: initialModel,
    deepSearch: false,
    browseWeb: false,
    responseMode: 'auto' as ResponseMode,
    modeAnnouncement: null,
    showSuggestions: false,
    isRecording: false,
    attachments: [],
    acState: null,
    acIndex: 0,
    entities: [],
  });
}
