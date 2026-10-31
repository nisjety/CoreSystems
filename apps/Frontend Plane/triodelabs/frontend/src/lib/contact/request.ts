import { z } from "zod";
import { contactSources } from "../../features/contact/contact-sources";
import { siteConfig } from "../constants";
import { locales } from "../i18n/locales";
import { contactFormSchema } from "../validations";

export const contactSubmissionSchema = contactFormSchema
  .extend({
    submissionId: z.string().uuid(),
    source: z.enum(contactSources),
    locale: z.enum(locales).optional(),
    website: z.literal(""),
  })
  .strict();

export type ContactSubmission = z.infer<typeof contactSubmissionSchema>;

export class ContactRequestError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export function validateRequestOrigin(request: Request): void {
  const origin = request.headers.get("origin");
  const canonicalOrigin = new URL(siteConfig.url).origin;
  const local = new URL(request.url);
  const localAllowed =
    process.env.NODE_ENV !== "production" &&
    ["localhost", "127.0.0.1", "[::1]"].includes(local.hostname) &&
    origin === local.origin;
  // The deployment hostname comes from server configuration, never request headers.
  const deploymentHost = process.env.VERCEL_URL;
  const previewAllowed =
    process.env.VERCEL === "1" &&
    deploymentHost?.endsWith(".vercel.app") &&
    origin === `https://${deploymentHost}`;
  if (origin !== canonicalOrigin && !localAllowed && !previewAllowed) {
    throw new ContactRequestError(
      403,
      "Henvendelsen må sendes fra kontaktsiden vår.",
    );
  }
}

export async function readContactSubmission(
  request: Request,
): Promise<ContactSubmission> {
  const contentType = request.headers
    .get("content-type")
    ?.split(";")[0]
    .trim()
    .toLowerCase();
  if (contentType !== "application/json") {
    throw new ContactRequestError(415, "Ugyldig format på henvendelsen.");
  }
  const maxBytes = 16 * 1024;
  if (Number(request.headers.get("content-length")) > maxBytes) {
    throw new ContactRequestError(413, "Henvendelsen er for stor.");
  }
  const reader = request.body?.getReader();
  if (!reader)
    throw new ContactRequestError(400, "Henvendelsen mangler innhold.");
  const decoder = new TextDecoder();
  let bytes = 0;
  let body = "";
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > maxBytes) {
        await reader.cancel();
        throw new ContactRequestError(413, "Henvendelsen er for stor.");
      }
      body += decoder.decode(chunk.value, { stream: true });
    }
    const parsed = contactSubmissionSchema.safeParse(
      JSON.parse(body + decoder.decode()),
    );
    if (!parsed.success)
      throw new ContactRequestError(400, "Kontroller feltene og prøv igjen.");
    return parsed.data;
  } catch (error) {
    if (error instanceof ContactRequestError) throw error;
    throw new ContactRequestError(400, "Kontroller feltene og prøv igjen.");
  } finally {
    reader.releaseLock();
  }
}
