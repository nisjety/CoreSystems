import { createSignal, For, onCleanup, Show } from "solid-js";
import {
  loadLearningStatus,
  type LearningStatus,
} from "@coresystem/learning-client";
import CoursesWorkspace from "./CoursesWorkspace";

const sections = [
  "Overview",
  "Courses",
  "Classroom",
  "Devices",
  "Settings",
] as const;
type Section = (typeof sections)[number];
const capabilityLabels: Record<string, string> = {
  "school-identity": "School sign-in",
  "course-persistence": "Course records & assignments",
  "learner-progress": "Attempt tracking (learner-reported)",
  "self-hosted-speech": "Private speech recognition",
  "tutor-runtime": "Guided learning",
  "teacher-assistance": "Teacher assistance",
};

export default function App() {
  const [section, setSection] = createSignal<Section>("Overview");
  const [status, setStatus] = createSignal<LearningStatus | undefined>(
    undefined,
  );
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal("");
  let controller: AbortController | undefined;
  onCleanup(() => controller?.abort());

  async function checkConnection() {
    controller?.abort();
    const current = new AbortController();
    controller = current;
    setLoading(true);
    setError("");
    setStatus(undefined);
    try {
      const result = await loadLearningStatus(current.signal);
      if (!current.signal.aborted) setStatus(result);
    } catch (cause) {
      if (!current.signal.aborted)
        setError(
          cause instanceof Error && cause.name !== "TimeoutError"
            ? cause.message
            : "The connection timed out. Check that Learning Core is running.",
        );
    } finally {
      if (!current.signal.aborted) setLoading(false);
    }
  }

  return (
    <div class="workspace">
      <aside class="sidebar" aria-label="Workspace navigation">
        <a class="brand" href="#" onClick={() => setSection("Overview")}>
          <span class="brand-mark" aria-hidden="true">
            t.
          </span>
          teachme
          <span class="brand-dot" aria-hidden="true">
            ✦
          </span>
        </a>
        <div class="workspace-label">
          <span class="workspace-icon" aria-hidden="true">
            T
          </span>
          <div>
            Teachme pilot<small>Workspace foundation</small>
          </div>
        </div>
        <p class="nav-label">YOUR WORKSPACE</p>
        <nav>
          <For each={sections}>
            {(item, index) => (
              <button
                class={section() === item ? "selected" : ""}
                aria-current={section() === item ? "page" : undefined}
                onClick={() => setSection(item)}
              >
                <span class="nav-number" aria-hidden="true">
                  0{index() + 1}
                </span>
                {item}
              </button>
            )}
          </For>
        </nav>
        <div class="sidebar-note">
          <span class="small-dot" aria-hidden="true" />
          <strong>Designed for your school</strong>
          <p>
            School-controlled data and learning that happens on the student’s
            own screen.
          </p>
        </div>
        <div class="sidebar-footer">
          PART OF CORESYSTEM<span>Windows pilot · 13–18</span>
        </div>
      </aside>
      <main>
        <header class="topbar">
          <span>
            Workspace <span class="slash">/</span> {section()}
          </span>
          <span class="pill">Foundation preview</span>
        </header>
        <div class="content">
          <Show
            when={section() === "Overview"}
            fallback={
              <Show
                when={section() === "Courses"}
                fallback={<section class="section-page">
                <p class="eyebrow">TEACHME / {section().toUpperCase()}</p>
                <h1>{section()}</h1>
                <p class="lead">
                  {section() === "Courses"
                    ? "Turn a learning goal into a guided experience."
                    : section() === "Classroom"
                      ? "Understand progress, and know when to step in."
                      : section() === "Devices"
                        ? "Connect the classroom with clear, visible permission."
                        : "Your school. Your processing boundary."}
                </p>
                <div class="empty-state">
                  <span class="empty-icon" aria-hidden="true">
                    ↗
                  </span>
                  <h2>
                    {section() === "Courses"
                      ? "Your first course starts here"
                      : section() === "Classroom"
                        ? "No school is connected yet"
                        : section() === "Devices"
                          ? "No devices are enrolled"
                          : "Strict self-hosted processing"}
                  </h2>
                  <p>
                    {section() === "Courses"
                      ? "Course publishing will open after school sign-in and durable course storage are connected. The first pilot will cover a reviewed PowerPoint lesson."
                      : section() === "Classroom"
                        ? "Class lists and learning progress will come from authorized school records. This preview contains no student data."
                        : section() === "Devices"
                          ? "Device enrollment and teacher assistance are not connected. Viewing, highlighting and control will each require scoped authorization."
                          : "Self-hosted Whisper is the selected speech-recognition direction. Speech, translation, narration and reasoning are separate connections; none is enabled yet."}
                  </p>
                  <button
                    class="button secondary"
                    onClick={() => setSection("Overview")}
                  >
                    View foundation status <span aria-hidden="true">↗</span>
                  </button>
                </div>
              </section>}
              >
                <CoursesWorkspace />
              </Show>
            }
          >
            <section class="intro">
              <div>
                <p class="eyebrow">A LITTLE GUIDANCE. A LOT OF POSSIBILITY.</p>
                <h1>
                  Learning, right
                  <br />
                  where it happens.
                </h1>
                <p class="lead">
                  Help students build confidence in real applications.
                  <br class="desktop-break" /> One clear step, one small
                  discovery at a time.
                </p>
              </div>
              <div class="intro-badge">
                <span aria-hidden="true">↗</span>
                <small>
                  Built around
                  <br />
                  the learner.
                </small>
              </div>
            </section>
            <div class="overview-grid">
              <section class="lesson-card">
                <div class="card-top">
                  <span class="pill dark">FIRST PILOT</span>
                  <span>01 / LEARN BY DOING</span>
                </div>
                <div class="lesson-art" aria-hidden="true">
                  <div class="slide">
                    <span class="slide-kicker">A GREAT IDEA STARTS HERE</span>
                    <strong>
                      Make your
                      <br />
                      ideas visible.
                    </strong>
                    <div class="slide-bars">
                      <i />
                      <i />
                      <i />
                    </div>
                  </div>
                  <div class="guide-bubble">
                    <span>✦</span> One step at a time.
                  </div>
                </div>
                <h2>
                  A first presentation.
                  <br />A new kind of confidence.
                </h2>
                <p>
                  A planned PowerPoint lesson with on-screen guidance, readable
                  captions and time to try it yourself.
                </p>
                <button
                  class="button light"
                  onClick={() => setSection("Courses")}
                >
                  Explore the course workspace <span aria-hidden="true">↗</span>
                </button>
              </section>
              <section class="status-card" aria-labelledby="status-title">
                <div class="card-top">
                  <p class="eyebrow">WORKSPACE SETUP</p>
                  <span class="small-dot amber" aria-hidden="true" />
                </div>
                <h2 id="status-title">
                  A foundation
                  <br />
                  we can build on.
                </h2>
                <p>
                  Check the local Learning Core service. School sign-in and live
                  teaching are still being connected.
                </p>
                <div class="status-message" role="status" aria-live="polite">
                  {loading()
                    ? "Checking Learning Core…"
                    : error() ||
                      (status()
                        ? "Learning Core connected · teaching not ready"
                        : "Connection not checked")}
                </div>
                <Show when={status()}>
                  {(connected) => (
                    <ul class="capabilities">
                      <For each={connected().capabilities}>
                        {(capability) => (
                          <li>
                            <span>{capabilityLabels[capability.key]}</span>
                            <small>Not connected</small>
                          </li>
                        )}
                      </For>
                    </ul>
                  )}
                </Show>
                <button
                  class="button primary"
                  disabled={loading()}
                  onClick={() => void checkConnection()}
                >
                  {loading() ? "Checking…" : "Check local service"}
                  <span aria-hidden="true">↗</span>
                </button>
                <small class="fine-print">
                  No microphone, screen or student data is accessed.
                </small>
              </section>
            </div>
            <section class="principles" aria-label="Learning principles">
              <div>
                <span>01</span>
                <h3>Try it yourself</h3>
                <p>Guidance supports the student’s own actions.</p>
              </div>
              <div>
                <span>02</span>
                <h3>Help, with permission</h3>
                <p>Teacher assistance should always be visible.</p>
              </div>
              <div>
                <span>03</span>
                <h3>Keep knowledge current</h3>
                <p>Reviewed course versions evolve with the tools.</p>
              </div>
            </section>
          </Show>
        </div>
      </main>
    </div>
  );
}
