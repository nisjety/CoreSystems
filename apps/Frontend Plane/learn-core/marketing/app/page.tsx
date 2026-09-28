export default function Home() {
  // Local foundation only. Production needs separately configured product origins.
  const dashboard = "http://127.0.0.1:5201";
  return (
    <>
      <header className="nav">
        <a className="brand" href="#">
          teachme<span>✦</span>
        </a>
        <nav aria-label="Main navigation">
          <a href="#how-it-works">The idea</a>
          <a href="#for-schools">For schools</a>
          <a className="nav-action" href={dashboard}>
            Open local workspace <span aria-hidden="true">↗</span>
          </a>
        </nav>
      </header>
      <main>
        <section className="hero">
          <div className="hero-copy">
            <p className="eyebrow">
              <span /> A WINDOWS PILOT FOR SCHOOLS · IN DEVELOPMENT
            </p>
            <h1>
              A little guidance.
              <br />A world of
              <br />
              <em>“I did that.”</em>
            </h1>
            <p className="intro">
              Meet Teachme. A learning companion designed to help students find
              their way through real applications, one thoughtful step at a
              time.
            </p>
            <a className="button" href="#how-it-works">
              Discover the idea <span aria-hidden="true">↗</span>
            </a>
            <p className="hero-note">
              For curious minds. And the teachers beside them.
            </p>
          </div>
          <div
            className="hero-illustration"
            aria-label="Illustration of a planned guided lesson"
          >
            <div className="orbit orbit-one" />
            <div className="orbit orbit-two" />
            <div className="floating-note note-top">
              Small steps.
              <br />
              <strong>Real confidence.</strong>
            </div>
            <div className="practice-window">
              <div className="window-chrome">
                <i />
                <i />
                <i />
                <span>YOUR NEXT DISCOVERY</span>
              </div>
              <div className="practice-content">
                <small>MY FIRST PRESENTATION</small>
                <h2>
                  Big ideas
                  <br />
                  start here.
                </h2>
                <div className="idea-shapes">
                  <div />
                  <div />
                  <div />
                </div>
                <span className="practice-caption">
                  A space to try. A chance to learn.
                </span>
              </div>
            </div>
            <div className="cursor-tip">
              <span aria-hidden="true">↖</span>
              <div>
                Let’s try this together.
                <small>One clear step, then your turn.</small>
              </div>
            </div>
            <span className="illustration-label">CONCEPT ILLUSTRATION</span>
          </div>
        </section>
        <section className="idea" id="how-it-works">
          <p className="eyebrow">LEARNING THAT MEETS YOU WHERE YOU ARE</p>
          <div className="section-heading">
            <h2>
              Less getting stuck.
              <br />
              More figuring it out.
            </h2>
            <p>
              The pilot will explore a simple rhythm: ask for help, follow a
              clear cue, and make the next discovery yourself.
            </p>
          </div>
          <div className="steps">
            <article>
              <span>01 / ASK</span>
              <h3>A question away.</h3>
              <p>
                Open the Windows companion with a shortcut and describe what you
                want to learn.
              </p>
            </article>
            <article>
              <span>02 / TRY</span>
              <h3>Your screen. Your turn.</h3>
              <p>
                Planned guidance combines pointing, readable captions and voice,
                with time for the student to act.
              </p>
            </article>
            <article>
              <span>03 / GROW</span>
              <h3>Make it your own.</h3>
              <p>
                Practice a similar task independently, so progress reflects
                understanding and growing confidence.
              </p>
            </article>
          </div>
        </section>
        <section className="school" id="for-schools">
          <div>
            <p className="eyebrow">
              BUILT WITH A SCHOOL’S RESPONSIBILITIES IN MIND
            </p>
            <h2>
              Teachers set the direction.
              <br />
              Students make the discoveries.
            </h2>
          </div>
          <div className="school-points">
            <p>
              <strong>Reviewed learning.</strong> Courses and revisions approved
              by their authors.
            </p>
            <p>
              <strong>Visible assistance.</strong> Separate permissions for
              viewing, guiding and controlling a device.
            </p>
            <p>
              <strong>School-controlled processing.</strong> A self-hosted
              architecture, including the planned Whisper speech-recognition
              connection.
            </p>
          </div>
        </section>
        <section className="pilot">
          <span className="pilot-symbol" aria-hidden="true">
            ✦
          </span>
          <p className="eyebrow">START SMALL. LEARN SOMETHING IMPORTANT.</p>
          <h2>
            One school. One first lesson.
            <br />A foundation for what comes next.
          </h2>
          <p>
            Teachme is being built for a supervised Windows pilot for ages
            13–18. School sign-in, live guidance and remote assistance are not
            available in this foundation preview.
          </p>
          <a className="button" href={dashboard}>
            Explore the local foundation <span aria-hidden="true">↗</span>
          </a>
        </section>
      </main>
      <footer>
        <a className="brand" href="#">
          teachme<span>✦</span>
        </a>
        <p>A learning product built on CoreSystem.</p>
        <span>Pilot in development · 2026</span>
      </footer>
    </>
  );
}
