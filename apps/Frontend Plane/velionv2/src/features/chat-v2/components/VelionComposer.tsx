export type ComposerToolId = "search" | "reason" | "research" | "image";

export type ComposerAttachment = {
  id: string;
  name: string;
  size: number;
  type: string;
  /** Object/data URL for the picked file — read to base64 before streaming
   *  (multimodal vision input). Optional; absent for metadata-only turns. */
  url?: string;
};

export type ComposerSubmitPayload = {
  text: string;
  tools: ComposerToolId[];
  attachments: ComposerAttachment[];
  model?: string;
};
