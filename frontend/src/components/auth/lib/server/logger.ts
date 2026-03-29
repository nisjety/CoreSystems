// Minimal structured logger for server-side (Next.js Route Handlers / Server Actions)
// JSON Lines output with consistent fields

type LogLevel = 'info' | 'warn' | 'error';

interface LogMeta {
  event?: string;
  userId?: string;
  organizationId?: string;
  ip?: string;
  ua?: string;
  [key: string]: unknown;
}

function log(level: LogLevel, message: string, meta: LogMeta = {}) {
  const entry = {
    ts: new Date().toISOString(),
    level,
    message,
    ...meta,
  };
  console.log(JSON.stringify(entry));
}

export const logger = {
  info: (message: string, meta?: LogMeta) => log('info', message, meta),
  warn: (message: string, meta?: LogMeta) => log('warn', message, meta),
  error: (message: string, meta?: LogMeta) => log('error', message, meta),
};

export default logger;