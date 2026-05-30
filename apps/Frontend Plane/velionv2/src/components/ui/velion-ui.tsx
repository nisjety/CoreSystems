import {
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type Ref,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from "react";
import { cn } from "@/lib/utils";

type ButtonVariant = "primary" | "secondary";
type ButtonSize = "md" | "sm" | "xs";
type ButtonRadius = "default" | "sm" | "pill";

const buttonVariantClass: Record<ButtonVariant, string> = {
  primary: "velion-button-primary",
  secondary: "velion-button-secondary",
};

const buttonSizeClass: Record<ButtonSize, string> = {
  md: "",
  sm: "velion-button-sm",
  xs: "velion-button-xs",
};

const buttonRadiusClass: Record<ButtonRadius, string> = {
  default: "",
  sm: "velion-button-radius-sm",
  pill: "velion-button-pill",
};

export function VelionButton({
  className,
  radius = "default",
  ref,
  size = "md",
  type = "button",
  variant = "secondary",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  radius?: ButtonRadius;
  ref?: Ref<HTMLButtonElement>;
  size?: ButtonSize;
  variant?: ButtonVariant;
}) {
  return (
    <button
      ref={ref}
      type={type}
      className={cn(
        "velion-button velion-ui-focus",
        buttonVariantClass[variant],
        buttonSizeClass[size],
        buttonRadiusClass[radius],
        className,
      )}
      {...props}
    />
  );
}

type IconButtonSize = "sm" | "xs" | "md" | "lg";
type IconButtonRadius = "default" | "sm" | "pill";

const iconButtonSizeClass: Record<IconButtonSize, string> = {
  sm: "",
  xs: "velion-icon-button-xs",
  md: "velion-icon-button-md",
  lg: "velion-icon-button-lg",
};

const iconButtonRadiusClass: Record<IconButtonRadius, string> = {
  default: "",
  sm: "velion-button-radius-sm",
  pill: "velion-icon-button-pill",
};

export function VelionIconButton({
  className,
  radius = "default",
  ref,
  size = "sm",
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  radius?: IconButtonRadius;
  ref?: Ref<HTMLButtonElement>;
  size?: IconButtonSize;
}) {
  return (
    <button
      ref={ref}
      type={type}
      className={cn(
        "velion-icon-button velion-ui-focus",
        iconButtonSizeClass[size],
        iconButtonRadiusClass[radius],
        className,
      )}
      {...props}
    />
  );
}

type FieldVariant = "default" | "compact" | "settings";

const fieldVariantClass: Record<FieldVariant, string> = {
  default: "velion-input",
  compact: "velion-field-compact",
  settings: "velion-settings-field",
};

export function VelionInput({
  className,
  ref,
  variant = "default",
  ...props
}: InputHTMLAttributes<HTMLInputElement> & {
  ref?: Ref<HTMLInputElement>;
  variant?: FieldVariant;
}) {
  return (
    <input
      ref={ref}
      className={cn(fieldVariantClass[variant], className)}
      {...props}
    />
  );
}

export function VelionSelect({
  className,
  ref,
  variant = "default",
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & {
  ref?: Ref<HTMLSelectElement>;
  variant?: FieldVariant;
}) {
  return (
    <select
      ref={ref}
      className={cn(fieldVariantClass[variant], className)}
      {...props}
    />
  );
}

type TextareaVariant = "default" | "compact" | "settings";

const textareaVariantClass: Record<TextareaVariant, string> = {
  default: "velion-textarea",
  compact: "velion-textarea-compact",
  settings: "velion-settings-field velion-settings-textarea",
};

export function VelionTextarea({
  className,
  ref,
  variant = "default",
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement> & {
  ref?: Ref<HTMLTextAreaElement>;
  variant?: TextareaVariant;
}) {
  return (
    <textarea
      ref={ref}
      className={cn(textareaVariantClass[variant], className)}
      {...props}
    />
  );
}

export function VelionSegmented({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("velion-segmented", className)} {...props}>
      {children}
    </div>
  );
}

export function VelionSegmentedButton({
  className,
  ref,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  ref?: Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      type={type}
      className={cn("velion-segmented-button velion-ui-focus", className)}
      {...props}
    />
  );
}

export function VelionModal({
  children,
  className,
  compact = false,
  label,
  size = "sm",
}: {
  children: ReactNode;
  className?: string;
  compact?: boolean;
  label: string;
  size?: "sm" | "md" | "search" | "wide";
}) {
  const sizeClass = {
    sm: "velion-modal-shell-sm",
    md: "velion-modal-shell-md",
    search: "velion-modal-shell-search",
    wide: "velion-modal-shell-wide",
  }[size];

  return (
    <dialog
      open
      className={cn("velion-modal-backdrop", compact ? "velion-modal-backdrop-compact" : "")}
      aria-label={label}
    >
      <div className={cn("velion-modal-shell", sizeClass, className)}>
        {children}
      </div>
    </dialog>
  );
}

export function VelionModalTitle({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h2 className={cn("velion-modal-title", className)} {...props}>
      {children}
    </h2>
  );
}

export function VelionModalClose({
  children = "Esc",
  className,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type={type} className={cn("velion-modal-close", className)} {...props}>
      {children}
    </button>
  );
}

export function VelionSwitch({
  checked,
  className,
  label,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  checked: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={cn(
        "relative h-7 w-12 shrink-0 rounded-full transition-colors",
        checked ? "bg-[#111111] dark:bg-white" : "bg-[#D7D8DA] dark:bg-white/18",
        className,
      )}
      {...props}
    >
      <span
        className={cn(
          "absolute left-0 top-1 size-5 rounded-full bg-white shadow-[0_1px_5px_rgba(17,17,17,0.18)] transition-transform dark:bg-[#111214]",
          checked ? "translate-x-6" : "translate-x-1",
        )}
      />
    </button>
  );
}
