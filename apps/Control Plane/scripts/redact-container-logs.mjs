#!/usr/bin/env node

const positiveInteger = (name, fallback) => {
  const value = Number.parseInt(process.env[name] ?? '', 10);
  return Number.isSafeInteger(value) && value > 0 ? value : fallback;
};

const maxInputBytes = positiveInteger('MAX_LOG_INPUT_BYTES', 128 * 1024);
const maxOutputBytes = positiveInteger('MAX_LOG_OUTPUT_BYTES', 32 * 1024);
const maxLines = positiveInteger('MAX_LOG_LINES', 80);

const chunks = [];
let inputBytes = 0;
for await (const chunk of process.stdin) {
  const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
  const remaining = maxInputBytes - inputBytes;
  if (remaining > 0) {
    const accepted = buffer.subarray(0, remaining);
    chunks.push(accepted);
    inputBytes += accepted.length;
  }
}

let output = Buffer.concat(chunks).toString('utf8');

// Remove complete and incomplete PEM material before line-oriented rules.
output = output.replace(
  /-----BEGIN [^-\r\n]+-----[\s\S]*?-----END [^-\r\n]+-----/gi,
  '[REDACTED PEM]',
);
output = output.replace(
  /-----BEGIN [^-\r\n]+-----[\s\S]*$/gi,
  '[REDACTED PEM]',
);

// Credentials embedded in supported broker/database URLs.
output = output.replace(
  /\b((?:postgres(?:ql)?|nats|redis|rediss):\/\/)[^\s/@]+@/gi,
  '$1[REDACTED]@',
);

// JSON log fields. Match compound/camel/snake keys containing a sensitive
// marker so accessToken and service_secret are covered as well.
output = output.replace(
  /("[^"\r\n]*(?:token|password|secret|private[_-]?key|api[_-]?key|authorization|cookie|x-service-auth)[^"\r\n]*"\s*:\s*")[^"\r\n]*(")/gi,
  '$1[REDACTED]$2',
);
output = output.replace(
  /('[^'\r\n]*(?:token|password|secret|private[_-]?key|api[_-]?key|authorization|cookie|x-service-auth)[^'\r\n]*'\s*:\s*')[^'\r\n]*(')/gi,
  '$1[REDACTED]$2',
);
// The bounded input may end inside a JSON value. Redact that incomplete tail
// too; requiring the closing quote would expose the retained secret prefix.
output = output.replace(
  /("[^"\r\n]*(?:token|password|secret|private[_-]?key|api[_-]?key|authorization|cookie|x-service-auth)[^"\r\n]*"\s*:\s*")[^"\r\n]*(?=$|\r?\n)/gim,
  '$1[REDACTED]',
);
output = output.replace(
  /('[^'\r\n]*(?:token|password|secret|private[_-]?key|api[_-]?key|authorization|cookie|x-service-auth)[^'\r\n]*'\s*:\s*')[^'\r\n]*(?=$|\r?\n)/gim,
  '$1[REDACTED]',
);

// Header-shaped diagnostics and bearer values.
output = output.replace(
  /^(\s*(?:authorization|proxy-authorization|cookie|set-cookie|x-service-auth|x-service-token|x-api-key)\s*:\s*).*$/gim,
  '$1[REDACTED]',
);
output = output.replace(/\b(Bearer)\s+[^\s,;]+/gi, '$1 [REDACTED]');

// Shell/config assignments and plain key/value log fields.
output = output.replace(
  /(\b[A-Za-z0-9_-]*(?:password|secret|token|private[_-]?key|api[_-]?key)[A-Za-z0-9_-]*\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s,;}]*)/gi,
  '$1[REDACTED]',
);

output = output.split(/\r?\n/).slice(0, maxLines).join('\n');
const encoded = Buffer.from(output);
if (encoded.length > maxOutputBytes) {
  output = encoded.subarray(0, maxOutputBytes).toString('utf8');
}
process.stdout.write(output);
