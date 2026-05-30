import { z } from "zod";

const fieldErrorSchema = z.object({
  field: z.string(),
  code: z.string(),
  message: z.string(),
});

export const apiErrorSchema = z.object({
  error: z.object({
    code: z.string(),
    message: z.string(),
    details: z.array(fieldErrorSchema).optional(),
    requestId: z.string().optional(),
  }),
});

export const cursorMetaSchema = z.object({
  limit: z.number().int().positive(),
  hasNext: z.boolean(),
  nextCursor: z.string().nullable(),
});

export type FieldError = z.infer<typeof fieldErrorSchema>;
export type ApiError = z.infer<typeof apiErrorSchema>;
export type CursorMeta = z.infer<typeof cursorMetaSchema>;

export interface ApiSuccess<T> {
  data: T;
  meta?: CursorMeta;
  links?: {
    self: string;
    next?: string;
  };
}

export function ok<T>(data: T, init?: Omit<ApiSuccess<T>, "data">): ApiSuccess<T> {
  return {
    data,
    ...init,
  };
}

export function cursorPage<T>({
  data,
  limit,
  nextCursor,
  self,
}: {
  data: T[];
  limit: number;
  nextCursor: string | null;
  self: string;
}): ApiSuccess<T[]> {
  return {
    data,
    meta: {
      limit,
      hasNext: nextCursor !== null,
      nextCursor,
    },
    links: {
      self,
      ...(nextCursor ? { next: `${self}${self.includes("?") ? "&" : "?"}cursor=${nextCursor}` } : {}),
    },
  };
}

export function fail({
  code,
  message,
  details,
  requestId,
}: {
  code: string;
  message: string;
  details?: FieldError[];
  requestId?: string;
}): ApiError {
  return {
    error: {
      code,
      message,
      ...(details ? { details } : {}),
      ...(requestId ? { requestId } : {}),
    },
  };
}
