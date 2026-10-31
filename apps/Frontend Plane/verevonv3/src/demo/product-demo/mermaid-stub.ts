/**
 * Build-time stand-in for mermaid (vite.demo.config.ts aliases 'mermaid' to this
 * file). Diagrams are not part of the demo, so rendering is refused, never loaded.
 */
const unavailable = 'Diagrammer er ikke med i demoen.'

function refuse(): Promise<never> {
  return Promise.reject(new Error(unavailable))
}

const mermaid = {
  initialize: (): void => undefined,
  render: refuse,
  parse: refuse,
  run: async (): Promise<void> => undefined,
  init: async (): Promise<void> => undefined,
  contentLoaded: (): void => undefined,
}

export const initialize = mermaid.initialize
export const render = mermaid.render
export const parse = mermaid.parse
export const run = mermaid.run
export const init = mermaid.init
export const contentLoaded = mermaid.contentLoaded

export default mermaid
