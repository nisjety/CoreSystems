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
  primary: "verevon-button-primary",
  secondary: "verevon-button-secondary",
};

const buttonSizeClass: Record<ButtonSize, string> = {
  md: "",
  sm: "verevon-button-sm",
  xs: "verevon-button-xs",
};

const buttonRadiusClass: Record<ButtonRadius, string> = {
  default: "",
  sm: "verevon-button-radius-sm",
  pill: "verevon-button-pill",
};

export function VerevonButton({
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
        "verevon-button verevon-ui-focus",
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
  xs: "verevon-icon-button-xs",
  md: "verevon-icon-button-md",
  lg: "verevon-icon-button-lg",
};

const iconButtonRadiusClass: Record<IconButtonRadius, string> = {
  default: "",
  sm: "verevon-button-radius-sm",
  pill: "verevon-icon-button-pill",
};

export function VerevonIconButton({
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
        "verevon-icon-button verevon-ui-focus",
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
  default: "verevon-input",
  compact: "verevon-field-compact",
  settings: "verevon-settings-field",
};

export function VerevonInput({
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

export function VerevonSelect({
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
  default: "verevon-textarea",
  compact: "verevon-textarea-compact",
  settings: "verevon-settings-field verevon-settings-textarea",
};

export function VerevonTextarea({
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

export function VerevonSegmented({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("verevon-segmented", className)} {...props}>
      {children}
    </div>
  );
}

export function VerevonSegmentedButton({
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
      className={cn("verevon-segmented-button verevon-ui-focus", className)}
      {...props}
    />
  );
}

export function VerevonModal({
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
    sm: "verevon-modal-shell-sm",
    md: "verevon-modal-shell-md",
    search: "verevon-modal-shell-search",
    wide: "verevon-modal-shell-wide",
  }[size];

  return (
    <dialog
      open
      className={cn("verevon-modal-backdrop", compact ? "verevon-modal-backdrop-compact" : "")}
      aria-label={label}
    >
      <div className={cn("verevon-modal-shell", sizeClass, className)}>
        {children}
      </div>
    </dialog>
  );
}

export function VerevonModalTitle({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h2 className={cn("verevon-modal-title", className)} {...props}>
      {children}
    </h2>
  );
}

export function VerevonModalClose({
  children = "Esc",
  className,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type={type} className={cn("verevon-modal-close", className)} {...props}>
      {children}
    </button>
  );
}

export function VerevonSwitch({
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
