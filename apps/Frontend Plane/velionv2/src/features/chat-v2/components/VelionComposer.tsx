export type ComposerToolId = "search" | "reason" | "research" | "image";

export type ComposerAttachment = {
  id: string;
  name: string;
  size: number;
  type: string;
};

export type ComposerSubmitPayload = {
  text: string;
  tools: ComposerToolId[];
  attachments: ComposerAttachment[];
};
