export function PreFooterStatementSection() {
  return (
    <section className="velion-prefooter" aria-label="Velion origin statement" data-prefooter-scroll>
      <div className="velion-prefooter__sticky">
        <div className="velion-prefooter__ambient" aria-hidden="true" data-prefooter-dark-panel />

        <div className="velion-prefooter__switch" data-prefooter-switch>
          <p className="velion-prefooter__phrase" data-prefooter-light-phrase>
            Every customer is different.
          </p>

          <p className="velion-prefooter__phrase velion-prefooter__phrase--dark" data-prefooter-dark-phrase>
            Your AI worker should be too.
          </p>
        </div>

        <div className="velion-prefooter__final" data-prefooter-final>
          <div className="velion-prefooter__inner">
            <p className="velion-prefooter__seal" aria-label="Velion copyright 2026. Designed for controlled AI work.">
              <span>VELION©2026</span>
              <span>DESIGNED FOR CONTROLLED AI WORK</span>
            </p>

            <span className="velion-prefooter__meta velion-prefooter__meta--left">OSLO</span>
            <span className="velion-prefooter__meta velion-prefooter__meta--right">GLOBAL</span>

            <div className="velion-prefooter__center">
              <p className="velion-prefooter__statement">
                VELION IS BUILT FOR COMPANIES THAT WANT AI TO DO REAL CUSTOMER WORK WITH SOURCES, APPROVALS, AUDIT
                HISTORY, AND HUMANS STILL IN CONTROL.
              </p>

              <div className="velion-prefooter__mark" aria-hidden="true">
                <span>V</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
