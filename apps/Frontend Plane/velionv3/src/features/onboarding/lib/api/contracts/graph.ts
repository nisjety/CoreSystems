export type PreviewResponse = {
  nodes: Array<{ id: string; label: string; group: string }>
  edges: Array<{ a: string; b: string }>
  counts: { nodes: number; edges: number; groups: number }
}

export function createEmptyPreviewResponse(): PreviewResponse {
  return { nodes: [], edges: [], counts: { nodes: 0, edges: 0, groups: 0 } }
}
