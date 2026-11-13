import { Show } from 'solid-js'

import { AutomationsList } from './AutomationsList'
import { EventTriggersSection } from './EventTriggersSection'

/**
 * "Automatiseringer" (EP-6-S1): the signed-in person's own schedules, filtered Active, Paused
 * and Expired. The full mount of the shared list (EP-6-S4): the rows, filters and WS20 guards
 * live in AutomationsList, which the chat panel and Settings mount too. The hub adds no server
 * behaviour of its own.
 *
 * Under the schedule list sits the event triggers section (W4-T8, F-t3-047), mounted only while
 * VITE_EVENT_TRIGGERS is the literal "true". Its own calls go to the gateway's trigger routes.
 */
function eventTriggersEnabled(): boolean {
  return import.meta.env.VITE_EVENT_TRIGGERS === 'true'
}

export function AutomationsHub() {
  return (
    <>
      <AutomationsList scope="mine" />
      <Show when={eventTriggersEnabled()}>
        <EventTriggersSection />
      </Show>
    </>
  )
}
