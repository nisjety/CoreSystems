export const capabilityKeys = [
  "school-identity",
  "course-persistence",
  "learner-progress",
  "self-hosted-speech",
  "tutor-runtime",
  "teacher-assistance",
] as const;
export type CapabilityKey = (typeof capabilityKeys)[number];
export interface LearningStatus {
  service: "learning-core";
  phase: "foundation" | "school-access-and-courses";
  ready: false;
  processingProfile: "strict-self-hosted";
  capabilities: Array<{ key: CapabilityKey; state: "not-connected" | "connected" }>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

// A connected school/course store does not imply speech, tutoring or teaching
// readiness. Unknown capabilities and states remain fail closed.
export function parseLearningStatus(envelope: unknown): LearningStatus {
  const data = isRecord(envelope) ? envelope.data : undefined;
  if (
    !isRecord(data) ||
    data.service !== "learning-core" ||
    (data.phase !== "foundation" &&
      data.phase !== "school-access-and-courses") ||
    data.ready !== false ||
    data.processingProfile !== "strict-self-hosted" ||
    !Array.isArray(data.capabilities)
  ) {
    throw new Error("Learning service returned an incompatible status.");
  }
  const keys = new Set<string>();
  for (const capability of data.capabilities) {
    if (
      !isRecord(capability) ||
      typeof capability.key !== "string" ||
      !capabilityKeys.some((key) => key === capability.key) ||
      (capability.state !== "not-connected" && capability.state !== "connected") ||
      keys.has(capability.key)
    ) {
      throw new Error("Learning service returned an incompatible status.");
    }
    keys.add(capability.key);
  }
  if (keys.size !== capabilityKeys.length)
    throw new Error("Learning service returned an incomplete status.");
  const states = new Map(
    data.capabilities.map((capability) => [
      (capability as { key: CapabilityKey }).key,
      (capability as { state: "connected" | "not-connected" }).state,
    ]),
  );
  if (
    (data.phase === "foundation" &&
      [...states.values()].some((state) => state !== "not-connected")) ||
    (data.phase === "school-access-and-courses" &&
      (states.get("school-identity") !== "connected" ||
        states.get("course-persistence") !== "connected" ||
        states.get("learner-progress") !== "connected"))
  ) {
    throw new Error("Learning service returned an inconsistent capability status.");
  }
  return data as unknown as LearningStatus;
}

export interface SchoolContext {
  user: { id: string };
  organization: { id: string; role: "owner" | "admin" | "member" | "viewer" };
}

export interface LearningClass {
  id: string;
  name: string;
  role: "teacher" | "student";
  createdAt: string;
}

export interface LearningClassMember {
  userId: string;
  role: "teacher" | "student";
  addedAt: string;
}

export interface LearningCourse {
  id: string;
  classId: string;
  revision: number;
  title: string;
  description: string;
  content: Record<string, unknown>;
  state: "draft" | "published";
  createdAt: string;
  publishedAt?: string;
}

export interface LearningAssignment {
  id: string;
  classId: string;
  courseId: string;
  revision: number;
  title: string;
  dueAt?: string;
  createdAt: string;
}

export interface LearningAttemptProgress {
  attemptId: string;
  learnerId: string;
  mode: "guide" | "demonstrate" | "practice";
  status: "in_progress" | "finished" | "abandoned";
  startedAt: string;
  finishedAt?: string;
  totalSteps: number;
  selfReportedCompleteSteps: number;
}

async function learningRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: "same-origin",
    cache: "no-store",
    headers: {
      "Content-Type": "application/json",
      "X-Learning-Request": "dashboard-v1",
      ...init?.headers,
    },
  });
  const payload = (await response.json().catch(() => undefined)) as {
    data?: T;
    error?: { message?: string };
  };
  if (!response.ok || payload?.data === undefined) {
    throw new Error(payload?.error?.message ?? "Learning service is unavailable.");
  }
  return payload.data;
}

export const getSchoolContext = () =>
  learningRequest<SchoolContext>("/api/v1/learning/me");

export const getLearningClasses = () =>
  learningRequest<LearningClass[]>("/api/v1/learning/classes");

export const createLearningClass = (name: string) =>
  learningRequest<LearningClass>("/api/v1/learning/classes", {
    method: "POST",
    body: JSON.stringify({ name }),
  });

export const addLearningClassMember = (
  classId: string,
  userId: string,
  role: "teacher" | "student",
) =>
  learningRequest<{ userId: string; role: string; status: string }>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/members`,
    { method: "POST", body: JSON.stringify({ userId, role }) },
  );

export const getLearningClassMembers = (classId: string) =>
  learningRequest<LearningClassMember[]>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/members`,
  );

export const removeLearningClassMember = (classId: string, userId: string) =>
  learningRequest<{ removed: boolean }>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/members/${encodeURIComponent(userId)}`,
    { method: "DELETE" },
  );

export const getLearningCourses = (classId: string) =>
  learningRequest<LearningCourse[]>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/courses`,
  );

export const createLearningCourse = (
  classId: string,
  course: Pick<LearningCourse, "title" | "description" | "content">,
) =>
  learningRequest<LearningCourse>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/courses`,
    { method: "POST", body: JSON.stringify(course) },
  );

export const createLearningCourseRevision = (
  classId: string,
  courseId: string,
  course: Pick<LearningCourse, "title" | "description" | "content">,
) =>
  learningRequest<LearningCourse>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/courses/${encodeURIComponent(courseId)}/revisions`,
    { method: "POST", body: JSON.stringify(course) },
  );

export const publishLearningCourse = (
  classId: string,
  courseId: string,
  revision: number,
) =>
  learningRequest<LearningCourse>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/courses/${encodeURIComponent(courseId)}/revisions/${revision}/publish`,
    { method: "POST", body: "{}" },
  );

export const createLearningAssignment = (
  classId: string,
  courseId: string,
  revision: number,
  dueAt?: string,
) =>
  learningRequest<LearningAssignment>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/assignments`,
    {
      method: "POST",
      body: JSON.stringify({ courseId, revision, ...(dueAt ? { dueAt } : {}) }),
    },
  );

export const getLearningAssignments = (classId: string) =>
  learningRequest<LearningAssignment[]>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/assignments`,
  );

export const getLearningAssignmentProgress = (classId: string, assignmentId: string) =>
  learningRequest<LearningAttemptProgress[]>(
    `/api/v1/learning/classes/${encodeURIComponent(classId)}/assignments/${encodeURIComponent(assignmentId)}/progress`,
  );

export async function loadLearningStatus(
  signal?: AbortSignal,
  fetcher: typeof fetch = fetch,
): Promise<LearningStatus> {
  const timeout = AbortSignal.timeout(5000);
  const response = await fetcher("/api/v1/learning/status", {
    credentials: "same-origin",
    cache: "no-store",
    signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
  });
  if (!response.ok)
    throw new Error(
      "Learning service is unavailable. Start learning-core and try again.",
    );
  return parseLearningStatus(await response.json());
}
