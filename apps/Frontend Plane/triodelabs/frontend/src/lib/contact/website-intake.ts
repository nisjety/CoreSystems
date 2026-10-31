// Server-only first-party delivery. Never import this module into a component.
import {
  constants,
  closeSync,
  fstatSync,
  openSync,
  readFileSync,
} from "node:fs";
import { isAbsolute } from "node:path";
import { z } from "zod";
import type { ContactSubmission } from "./request";

type IntakeConfig = { url: string; token: string };
export function usesWebsiteIntake(): boolean {
  return process.env.CONTACT_DELIVERY_MODE === "leads";
}
export function getWebsiteIntakeConfig(): IntakeConfig | null {
  if (!usesWebsiteIntake()) return null;
  let descriptor: number | undefined;
  try {
    const url = new URL(process.env.CONTACT_WEBSITE_INTAKE_URL ?? "");
    const local =
      process.env.NODE_ENV === "development" &&
      url.protocol === "http:" &&
      ["localhost", "127.0.0.1"].includes(url.hostname);
    if (
      (!local && url.protocol !== "https:") ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== "/api/v1/website-inquiries"
    )
      return null;
    const path = process.env.CONTACT_WEBSITE_INTAKE_TOKEN_FILE ?? "";
    if (!isAbsolute(path)) return null;
    descriptor = openSync(
      path,
      constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
    );
    const info = fstatSync(descriptor);
    if (!info.isFile() || (info.mode & 0o077) !== 0 || info.size > 512)
      return null;
    const token = readFileSync(descriptor, "utf8").trim();
    if (!/^[A-Za-z0-9_-]{32,256}$/.test(token)) return null;
    return { url: url.href, token };
  } catch {
    return null;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

export async function deliverWebsiteInquiry(
  config: IntakeConfig,
  submission: ContactSubmission,
) {
  const fields = {
    submissionId: submission.submissionId,
    name: submission.name,
    email: submission.email,
    company: submission.company,
    projectType: submission.projectType,
    budget: submission.budget,
    message: submission.message,
    source: submission.source,
    locale: submission.locale,
  };
  const response = await fetch(config.url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${config.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(fields),
    signal: AbortSignal.timeout(8000),
    redirect: "error",
    cache: "no-store",
  });
  if (!response.ok)
    throw new Error(
      response.status === 409 ? "submission_conflict" : "inquiry_unconfirmed",
    );
  const result = z
    .object({
      data: z.object({
        receiptId: z.string().regex(/^plead_[a-f0-9]{24}$/),
        acceptedAt: z.string().datetime({ offset: true }),
      }),
    })
    .safeParse(await response.json());
  if (!result.success) throw new Error("inquiry_unconfirmed");
  return result.data.data;
}
