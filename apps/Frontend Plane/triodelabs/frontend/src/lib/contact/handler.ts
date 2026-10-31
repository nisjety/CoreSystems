import {
  deliverWebsiteInquiry,
  getWebsiteIntakeConfig,
  usesWebsiteIntake,
} from "./website-intake";
import { getContactConfig } from "./config";
import {
  ContactRequestError,
  readContactSubmission,
  validateRequestOrigin,
} from "./request";
import { sendContactEmail } from "./send";
import {
  checkRateLimit,
  getReceipt,
  recordLead,
  submissionFingerprint,
} from "./storage";

function failure(
  status: number,
  error: string,
  headers: Record<string, string> = {},
) {
  return Response.json(
    { success: false, error },
    { status, headers: { "Cache-Control": "no-store", ...headers } },
  );
}

const unavailable = () =>
  failure(
    503,
    "Skjemaet er midlertidig utilgjengelig. Kontakt oss på post@triodelab.no.",
  );
const success = () =>
  Response.json(
    { success: true },
    { headers: { "Cache-Control": "no-store" } },
  );

export async function handleContactRequest(
  request: Request,
): Promise<Response> {
  if (request.method !== "POST")
    return failure(405, "Metoden støttes ikke.", { Allow: "POST" });
  let submission;
  try {
    validateRequestOrigin(request);
    submission = await readContactSubmission(request);
  } catch (error) {
    if (error instanceof ContactRequestError)
      return failure(error.status, error.message);
    return failure(400, "Kontroller feltene og prøv igjen.");
  }
  if (usesWebsiteIntake()) {
    const intake = getWebsiteIntakeConfig();
    if (!intake) return unavailable();
    try {
      const receipt = await deliverWebsiteInquiry(intake, submission);
      return Response.json(
        { success: true, receiptId: receipt.receiptId },
        { headers: { "Cache-Control": "no-store" } },
      );
    } catch (error) {
      if (error instanceof Error && error.message === "submission_conflict")
        return failure(
          409,
          "Innholdet er endret. Oppdater siden og prøv igjen.",
        );
      return failure(
        502,
        "Vi kunne ikke bekrefte innsendingen. Prøv igjen senere.",
      );
    }
  }
  const config = getContactConfig();
  if (!config) return unavailable();

  const redis = config.redis;
  if (redis) {
    try {
      const retryAfter = await checkRateLimit(redis, request);
      if (retryAfter !== null) {
        return failure(
          429,
          "Du har sendt flere henvendelser på kort tid. Prøv igjen senere.",
          { "Retry-After": String(retryAfter) },
        );
      }
      const receipt = await getReceipt(redis, submission.submissionId);
      if (receipt) {
        return receipt.fingerprint === submissionFingerprint(redis, submission)
          ? success()
          : failure(
              409,
              "Innholdet er endret. Oppdater siden og send henvendelsen på nytt.",
            );
      }
    } catch {
      // A broken Upstash setup must not stop real enquiries from arriving.
      // Resend's idempotency key still prevents duplicate emails.
      console.error("contact_rate_limit_unavailable");
    }
  } else {
    console.error("contact_rate_limit_not_configured");
  }

  let providerId: string;
  try {
    providerId = await sendContactEmail(config, submission);
  } catch {
    console.error("contact_delivery_unconfirmed");
    return failure(
      502,
      "Vi kunne ikke bekrefte innsendingen. Prøv igjen eller kontakt post@triodelab.no.",
    );
  }
  if (!redis) return success();
  try {
    await recordLead(redis, submission, providerId);
  } catch {
    // Email has been accepted; a metrics outage must not report a delivery failure.
    console.error("contact_metrics_unavailable");
  }
  return success();
}
