import { z } from "zod";
import type { ContactConfig } from "./config";
import { buildContactEmail } from "./email-template";
import type { ContactSubmission } from "./request";

export async function sendContactEmail(
  config: ContactConfig,
  submission: ContactSubmission,
): Promise<string> {
  const { html, subject, text } = buildContactEmail(submission);
  const response = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${config.apiKey}`,
      "Content-Type": "application/json",
      "Idempotency-Key": `contact/${submission.submissionId}`,
    },
    body: JSON.stringify({
      from: config.from,
      to: [config.to],
      reply_to: submission.email,
      subject,
      html,
      text,
    }),
    signal: AbortSignal.timeout(8000),
    cache: "no-store",
  });
  if (!response.ok) throw new Error("contact_delivery_failed");
  const accepted = z
    .object({ id: z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/) })
    .safeParse(await response.json());
  if (!accepted.success) throw new Error("contact_delivery_unconfirmed");
  return accepted.data.id;
}
