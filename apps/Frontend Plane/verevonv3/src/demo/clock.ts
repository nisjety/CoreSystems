/**
 * The fictional clock of the product scenes and the interactive demo.
 *
 * `new Date()` and `Date.now()` start at the given instant and then advance
 * with `performance.now()`, so the greeting, relative labels and elapsed waits
 * behave, while the story's absolute dates («onsdag 14. okt.») stay true.
 * Under the capture harness `performance.now()` follows virtual time, so films
 * stay frame-identical. Date arguments are never touched: `new Date(2026, 1, 1)`
 * and `Date.parse` behave as usual.
 *
 * The films install the same patch inline (scenes/02-plan.ts, 03-undersok.ts);
 * this module is the shared version for the demo entry.
 */
export function installFixedClock(iso: string): void {
  if (typeof window === 'undefined') return
  const RealDate = Date
  if ((RealDate as unknown as { __verevonSceneFixed?: boolean }).__verevonSceneFixed) return
  const fixedMs = RealDate.parse(iso)
  if (!Number.isFinite(fixedMs)) throw new Error(`installFixedClock: invalid date ${iso}`)
  const perfAtInstall = performance.now()
  const sceneNow = () => fixedMs + (performance.now() - perfAtInstall)
  class SceneDate extends RealDate {
    constructor(...args: unknown[]) {
      if (args.length === 0) super(sceneNow())
      else super(...(args as [string | number | Date]))
    }
    static override now(): number {
      return Math.floor(sceneNow())
    }
  }
  Object.defineProperty(SceneDate, '__verevonSceneFixed', { value: true })
  // `Date()` called as a function returns a string of "now"; keep that too.
  window.Date = new Proxy(SceneDate, {
    apply: () => new SceneDate().toString(),
  }) as unknown as DateConstructor
}
