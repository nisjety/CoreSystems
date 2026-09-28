import { For, Show, createSignal } from "solid-js";
import {
  addLearningClassMember,
  createLearningAssignment,
  createLearningClass,
  createLearningCourse,
  createLearningCourseRevision,
  getLearningAssignments,
  getLearningAssignmentProgress,
  getLearningClasses,
  getLearningCourses,
  getLearningClassMembers,
  getSchoolContext,
  publishLearningCourse,
  removeLearningClassMember,
  type LearningAssignment,
  type LearningClass,
  type LearningClassMember,
  type LearningCourse,
  type LearningAttemptProgress,
  type SchoolContext,
} from "@coresystem/learning-client";

const starterDefinition = JSON.stringify(
  {
    version: 1,
    application: "Microsoft PowerPoint",
    objectives: ["Create and present a clear title slide"],
    steps: [
      {
        id: "title-slide",
        instruction: "Add a title and subtitle to the first slide.",
        expectedResult: "The opening slide shows both a title and a subtitle.",
      },
    ],
  },
  null,
  2,
);

export default function CoursesWorkspace() {
  let classLoadSequence = 0;
  const [school, setSchool] = createSignal<SchoolContext>();
  const [classes, setClasses] = createSignal<LearningClass[]>([]);
  const [members, setMembers] = createSignal<LearningClassMember[]>([]);
  const [selectedClass, setSelectedClass] = createSignal("");
  const [courses, setCourses] = createSignal<LearningCourse[]>([]);
  const [assignments, setAssignments] = createSignal<LearningAssignment[]>([]);
  const [assignmentProgress, setAssignmentProgress] = createSignal<Record<string, LearningAttemptProgress[]>>({});
  const [openProgress, setOpenProgress] = createSignal("");
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [className, setClassName] = createSignal("");
  const [memberID, setMemberID] = createSignal("");
  const [memberRole, setMemberRole] = createSignal<"teacher" | "student">(
    "student",
  );
  const [courseTitle, setCourseTitle] = createSignal("");
  const [revisionCourseID, setRevisionCourseID] = createSignal("");
  const [courseDescription, setCourseDescription] = createSignal("");
  const [courseContent, setCourseContent] = createSignal(starterDefinition);
  const [error, setError] = createSignal("");
  const [message, setMessage] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [authChecking, setAuthChecking] = createSignal(true);

  void refreshSchool();

  async function refreshSchool() {
    setAuthChecking(true);
    setError("");
    try {
      const context = await getSchoolContext();
      setSchool(context);
      const nextClasses = await getLearningClasses();
      setClasses(nextClasses);
      const selected =
        nextClasses.find((item) => item.id === selectedClass())?.id ??
        nextClasses[0]?.id ??
        "";
      setSelectedClass(selected);
      await loadClass(selected);
    } catch (cause) {
      setSchool(undefined);
      setClasses([]);
      setMembers([]);
      setSelectedClass("");
      setCourses([]);
      setAssignments([]);
      setAssignmentProgress({});
      setOpenProgress("");
      const reason = cause instanceof Error ? cause.message : "";
      if (!reason.toLowerCase().includes("sign in")) setError(reason);
    } finally {
      setAuthChecking(false);
    }
  }

  async function loadClass(classID: string) {
    const requestSequence = ++classLoadSequence;
    setSelectedClass(classID);
    setCourses([]);
    setAssignments([]);
    setAssignmentProgress({});
    setOpenProgress("");
    setMembers([]);
    if (!classID) return;
    try {
      const currentClass = classes().find((item) => item.id === classID);
      const [nextCourses, nextAssignments, nextMembers] = await Promise.all([
        getLearningCourses(classID),
        getLearningAssignments(classID),
        currentClass?.role === "teacher"
          ? getLearningClassMembers(classID)
          : Promise.resolve([]),
      ]);
      if (requestSequence !== classLoadSequence) return;
      setCourses(nextCourses);
      setAssignments(nextAssignments);
      setMembers(nextMembers);
    } catch (cause) {
      if (requestSequence === classLoadSequence) throw cause;
    }
  }

  async function signIn(event: SubmitEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const response = await fetch("/api/auth/sign-in/email", {
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: email().trim(), password: password() }),
      });
      const payload = (await response.json()) as {
        error?: { message?: string } | string;
      };
      if (!response.ok || payload.error) {
        throw new Error("Sign-in failed. Check your account, password, and allowed app origin.");
      }
      setPassword("");
      await refreshSchool();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Control Plane sign-in failed.");
    } finally {
      setBusy(false);
    }
  }

  async function signOut() {
    setBusy(true);
    setError("");
    try {
      const response = await fetch("/api/auth/sign-out", {
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
      });
      if (!response.ok) throw new Error("Control Plane did not end the school session.");
      setSchool(undefined);
      setClasses([]);
      setMembers([]);
      setSelectedClass("");
      setCourses([]);
      setAssignments([]);
      setAssignmentProgress({});
      setOpenProgress("");
    } catch {
      setError("Could not sign out of Control Plane.");
    } finally {
      setBusy(false);
    }
  }

  async function createClass(event: SubmitEvent) {
    event.preventDefault();
    await run(async () => {
      const created = await createLearningClass(className().trim());
      setClassName("");
      await refreshSchool();
      await loadClass(created.id);
      setMessage("Class created. You are its first teacher.");
    });
  }

  async function addMember(event: SubmitEvent) {
    event.preventDefault();
    const classID = selectedClass();
    if (!classID) return;
    await run(async () => {
      await addLearningClassMember(classID, memberID().trim(), memberRole());
      setMemberID("");
      await loadClass(classID);
      setMessage("Verified school member added to this class.");
    });
  }

  async function removeMember(userId: string) {
    const classID = selectedClass();
    await run(async () => {
      await removeLearningClassMember(classID, userId);
      await loadClass(classID);
      setMessage("Class membership removed.");
    });
  }

  async function createCourse(event: SubmitEvent) {
    event.preventDefault();
    const classID = selectedClass();
    if (!classID) return;
    await run(async () => {
      let content: Record<string, unknown>;
      try {
        const parsed: unknown = JSON.parse(courseContent());
        if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed))
          throw new Error();
        content = parsed as Record<string, unknown>;
      } catch {
        throw new Error("Course definition must be a JSON object.");
      }
      const input = {
        title: courseTitle().trim(),
        description: courseDescription().trim(),
        content,
      };
      if (revisionCourseID()) {
        await createLearningCourseRevision(classID, revisionCourseID(), input);
      } else {
        await createLearningCourse(classID, input);
      }
      setCourseTitle("");
      setCourseDescription("");
      setRevisionCourseID("");
      await loadClass(classID);
      setMessage("Draft revision saved. Publishing locks it for assigned students.");
    });
  }

  async function publish(course: LearningCourse) {
    const classID = selectedClass();
    await run(async () => {
      await publishLearningCourse(classID, course.id, course.revision);
      await loadClass(classID);
      setMessage(`Revision ${course.revision} is published and ready to assign.`);
    });
  }

  function prepareRevision(course: LearningCourse) {
    setRevisionCourseID(course.id);
    setCourseTitle(course.title);
    setCourseDescription(course.description);
    setCourseContent(JSON.stringify(course.content, null, 2));
    document.querySelector(".course-editor")?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  function isLatestRevision(course: LearningCourse) {
    return course.revision === Math.max(
      ...courses()
        .filter((item) => item.id === course.id)
        .map((item) => item.revision),
    );
  }

  async function assign(course: LearningCourse) {
    await run(async () => {
      await createLearningAssignment(
        selectedClass(),
        course.id,
        course.revision,
      );
      await loadClass(selectedClass());
      setMessage(`“${course.title}” revision ${course.revision} assigned to this class.`);
    });
  }

  async function inspectProgress(assignment: LearningAssignment) {
    const classID = selectedClass();
    if (openProgress() === assignment.id) {
      setOpenProgress("");
      return;
    }
    await run(async () => {
      const progress = await getLearningAssignmentProgress(classID, assignment.id);
      setAssignmentProgress((current) => ({ ...current, [assignment.id]: progress }));
      setOpenProgress(assignment.id);
    });
  }

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The learning change could not be saved.");
    } finally {
      setBusy(false);
    }
  }

  const activeClass = () => classes().find((item) => item.id === selectedClass());
  const canCreateClass = () =>
    school()?.organization.role === "owner" ||
    school()?.organization.role === "admin";

  return (
    <section class="section-page courses-workspace">
      <div class="courses-heading">
        <div>
          <p class="eyebrow">TEACHME / COURSES</p>
          <h1>Your course workspace</h1>
          <p class="lead">
            Sign in through Control, then build a course for a class in your school.
          </p>
        </div>
        <Show when={school()}>
          <button class="button secondary" disabled={busy()} onClick={signOut}>
            Sign out
          </button>
        </Show>
      </div>

      <Show when={authChecking()}>
        <div class="workspace-panel" role="status">Checking your school session…</div>
      </Show>

      <Show when={!authChecking() && !school()}>
        <form class="workspace-panel sign-in-panel" onSubmit={signIn}>
          <span class="pill dark">CONTROL PLANE</span>
          <h2>Sign in with your school account</h2>
          <p>Your account and school membership stay managed by CoreSystem Control.</p>
          <label>
            Email
            <input
              type="email"
              autocomplete="username"
              required
              value={email()}
              onInput={(event) => setEmail(event.currentTarget.value)}
            />
          </label>
          <label>
            Password
            <input
              type="password"
              autocomplete="current-password"
              required
              value={password()}
              onInput={(event) => setPassword(event.currentTarget.value)}
            />
          </label>
          <button class="button dark-button" disabled={busy()}>
            {busy() ? "Signing in…" : "Sign in"}
          </button>
          <p class="muted-note">
            Accounts must already exist in Control. Teachme does not create or store passwords.
          </p>
        </form>
      </Show>

      <Show when={school()}>
        {(context) => (
          <div class="workspace-stack">
            <div class="workspace-panel school-context">
              <div>
                <span class="eyebrow">ACTIVE SCHOOL</span>
                <strong>{context().organization.id}</strong>
                <span class="muted-note">Control role: {context().organization.role}</span>
              </div>
              <span class="pill">Membership verified</span>
            </div>

            <Show when={classes().length > 0}>
              <label class="class-picker">
                Active class
                <select
                  value={selectedClass()}
                  disabled={busy()}
                  onChange={(event) => void run(() => loadClass(event.currentTarget.value))}
                >
                  <For each={classes()}>
                    {(classroom) => (
                      <option value={classroom.id}>
                        {classroom.name} · {classroom.role}
                      </option>
                    )}
                  </For>
                </select>
              </label>
            </Show>

            <Show when={canCreateClass()}>
              <form class="workspace-panel inline-form" onSubmit={createClass}>
                <div>
                  <span class="eyebrow">SCHOOL SETUP</span>
                  <h2>Create a class</h2>
                  <p>The signed-in school administrator becomes its first teacher.</p>
                </div>
                <label>
                  Class name
                  <input
                    required
                    maxlength="120"
                    value={className()}
                    onInput={(event) => setClassName(event.currentTarget.value)}
                    placeholder="Year 10 · Digital skills"
                  />
                </label>
                <button class="button dark-button" disabled={busy()}>Create class</button>
              </form>
            </Show>

            <Show when={!classes().length && !canCreateClass()}>
              <div class="workspace-panel empty-course-state">
                <h2>No class access yet</h2>
                <p>A school administrator must create a class and assign your teacher or student role.</p>
              </div>
            </Show>

            <Show when={activeClass()}>
              <div class="workspace-panel class-summary">
                <div>
                  <span class="eyebrow">CLASSROOM</span>
                  <h2>{activeClass()?.name}</h2>
                  <p>You are enrolled as a {activeClass()?.role}.</p>
                </div>
                <span class="pill">School scoped</span>
              </div>

              <Show when={activeClass()?.role === "teacher"}>
                <div class="workspace-panel roster-panel">
                  <div class="course-list-heading">
                    <div>
                      <span class="eyebrow">CLASS ROSTER</span>
                      <h2>{members().length} active members</h2>
                    </div>
                  </div>
                  <For each={members()}>
                    {(member) => (
                      <div class="roster-row">
                        <div>
                          <strong>{member.role === "teacher" ? "Teacher" : "Student"}</strong>
                          <span>{member.userId}</span>
                        </div>
                        <span class="pill">{member.role}</span>
                        <Show when={canCreateClass() || member.userId === school()?.user.id}>
                          <button
                            class="text-button"
                            disabled={busy() || (member.role === "teacher" && members().filter((entry) => entry.role === "teacher").length === 1)}
                            onClick={() => void removeMember(member.userId)}
                          >
                            {member.userId === school()?.user.id ? "Leave class" : "Remove"}
                          </button>
                        </Show>
                      </div>
                    )}
                  </For>
                </div>
                <form class="workspace-panel member-form" onSubmit={addMember}>
                  <div>
                    <span class="eyebrow">CLASS ACCESS</span>
                    <h2>Add a school member</h2>
                    <p>Control verifies that the account is currently active in this school.</p>
                  </div>
                  <label>
                    Control user ID
                    <input
                      required
                      value={memberID()}
                      onInput={(event) => setMemberID(event.currentTarget.value)}
                      placeholder="Existing school account ID"
                    />
                  </label>
                  <Show when={canCreateClass()}>
                    <label>
                      Class role
                      <select
                        value={memberRole()}
                        onChange={(event) => setMemberRole(event.currentTarget.value as "teacher" | "student")}
                      >
                        <option value="student">Student</option>
                        <option value="teacher">Teacher</option>
                      </select>
                    </label>
                  </Show>
                  <button class="button secondary" disabled={busy()}>Add member</button>
                </form>
              </Show>

              <Show when={activeClass()?.role === "teacher"}>
                <form class="workspace-panel course-editor" onSubmit={createCourse}>
                  <div>
                    <span class="eyebrow">COURSE AUTHORING</span>
                    <h2>{revisionCourseID() ? "Create the next revision" : "Create a course draft"}</h2>
                    <p>Published revisions are immutable; later changes become a new revision.</p>
                  </div>
                  <label>
                    Course title
                    <input
                      required
                      maxlength="160"
                      value={courseTitle()}
                      onInput={(event) => setCourseTitle(event.currentTarget.value)}
                      placeholder="Create your first presentation"
                    />
                  </label>
                  <label>
                    Short description
                    <textarea
                      maxlength="2000"
                      rows="2"
                      value={courseDescription()}
                      onInput={(event) => setCourseDescription(event.currentTarget.value)}
                      placeholder="What will learners practise?"
                    />
                  </label>
                  <label>
                    Version 1 course definition (JSON)
                    <textarea
                      class="course-json"
                      required
                      rows="12"
                      spellcheck={false}
                      value={courseContent()}
                      onInput={(event) => setCourseContent(event.currentTarget.value)}
                    />
                  </label>
                  <p class="muted-note">
                    This records reviewed lesson content. It does not yet drive Windows guidance or verify learner outcomes.
                  </p>
                  <div class="authoring-actions">
                    <button class="button dark-button" disabled={busy()}>
                      {revisionCourseID() ? "Save next revision" : "Save course draft"}
                    </button>
                    <Show when={revisionCourseID()}>
                      <button
                        class="button secondary"
                        type="button"
                        disabled={busy()}
                        onClick={() => {
                          setRevisionCourseID("");
                          setCourseTitle("");
                          setCourseDescription("");
                          setCourseContent(starterDefinition);
                        }}
                      >
                        Cancel revision
                      </button>
                    </Show>
                  </div>
                </form>
              </Show>

              <div class="course-list-heading">
                <div>
                  <span class="eyebrow">CLASS CONTENT</span>
                  <h2>{activeClass()?.role === "student" ? "Assigned courses" : "Courses and revisions"}</h2>
                </div>
              </div>
              <Show when={!courses().length}>
                <div class="workspace-panel empty-course-state">
                  <h3>{activeClass()?.role === "student" ? "No courses assigned" : "No course drafts yet"}</h3>
                  <p>{activeClass()?.role === "student" ? "Your teacher’s published assignments will appear here." : "Create the first course draft for this class above."}</p>
                </div>
              </Show>
              <div class="course-card-list">
                <For each={courses()}>
                  {(course) => (
                    <article class="workspace-panel course-record">
                      <div class="course-record-heading">
                        <div>
                          <span class="eyebrow">REVISION {course.revision}</span>
                          <h3>{course.title}</h3>
                          <p>{course.description || "No description provided."}</p>
                        </div>
                        <span class={`pill ${course.state === "published" ? "dark" : ""}`}>
                          {course.state}
                        </span>
                      </div>
                      <details>
                        <summary>Review lesson definition</summary>
                        <pre>{JSON.stringify(course.content, null, 2)}</pre>
                      </details>
                      <Show when={activeClass()?.role === "teacher" && course.state === "draft"}>
                        <button class="button secondary" disabled={busy()} onClick={() => void publish(course)}>
                          Publish revision
                        </button>
                      </Show>
                      <Show when={activeClass()?.role === "teacher" && course.state === "published"}>
                        <div class="course-record-actions">
                          <Show when={isLatestRevision(course)}>
                            <button class="button secondary" disabled={busy()} onClick={() => prepareRevision(course)}>
                              Draft next revision
                            </button>
                          </Show>
                          <button class="button light" disabled={busy()} onClick={() => void assign(course)}>
                            Assign this revision
                          </button>
                        </div>
                      </Show>
                    </article>
                  )}
                </For>
              </div>

              <div class="course-list-heading">
                <div>
                  <span class="eyebrow">ASSIGNMENTS</span>
                  <h2>Course versions assigned to this class</h2>
                </div>
              </div>
              <Show when={!assignments().length}>
                <div class="workspace-panel empty-course-state">
                  <p>No course versions have been assigned to this class.</p>
                </div>
              </Show>
              <div class="assignment-list">
                <For each={assignments()}>
                  {(assignment) => (
                    <div class="workspace-panel assignment-record">
                      <span class="assignment-check" aria-hidden="true">01</span>
                      <div>
                        <strong>{assignment.title}</strong>
                        <span>Revision {assignment.revision} · assigned {new Date(assignment.createdAt).toLocaleDateString()}</span>
                      </div>
                      <Show when={assignment.dueAt}>
                        <span class="pill">Due {new Date(assignment.dueAt!).toLocaleDateString()}</span>
                      </Show>
                      <Show when={activeClass()?.role === "teacher"}>
                        <button class="button light" disabled={busy()} onClick={() => void inspectProgress(assignment)}>
                          {openProgress() === assignment.id ? "Hide progress" : "View progress"}
                        </button>
                      </Show>
                      <Show when={openProgress() === assignment.id}>
                        <div class="assignment-progress" role="status">
                          <strong>Attempt activity</strong>
                          <p>Student-reported steps only. These counts do not verify independent mastery.</p>
                          <Show when={(assignmentProgress()[assignment.id] ?? []).length > 0} fallback={<p>No learning attempts have been recorded for this assignment.</p>}>
                            <For each={assignmentProgress()[assignment.id] ?? []}>
                              {(item) => (
                                <div class="assignment-progress-row">
                                  <span>Student · {item.learnerId}</span>
                                  <span>{item.status.replace("_", " ")}</span>
                                  <span>{item.selfReportedCompleteSteps} of {item.totalSteps} steps self-reported complete</span>
                                </div>
                              )}
                            </For>
                          </Show>
                        </div>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </div>
        )}
      </Show>

      <Show when={message()}>
        <p class="workspace-feedback success" role="status">{message()}</p>
      </Show>
      <Show when={error()}>
        <p class="workspace-feedback failure" role="alert">{error()}</p>
      </Show>
    </section>
  );
}
