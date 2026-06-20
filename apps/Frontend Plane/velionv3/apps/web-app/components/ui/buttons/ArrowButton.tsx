import type { ReactNode } from "react";

type ArrowButtonProps = {
  children: ReactNode;
  className?: string;
  href?: string;
  onClick?: () => void;
  type?: "button" | "submit" | "reset";
  variant?: "dark" | "light" | "muted";
};

export function ArrowButton({
  children,
  className = "",
  href,
  onClick,
  type = "button",
  variant = "dark",
}: ArrowButtonProps) {
  const classes = ["velion-arrow-button", `velion-arrow-button--${variant}`, className]
    .filter(Boolean)
    .join(" ");

  const content = (
    <>
      <span>{children}</span>
      <svg aria-hidden="true" viewBox="0 0 24 12">
        <path d="M1 6h20M16 1l5 5-5 5" />
      </svg>
    </>
  );

  if (href) {
    return (
      <a className={classes} href={href}>
        {content}
      </a>
    );
  }

  return (
    <button className={classes} onClick={onClick} type={type}>
      {content}
    </button>
  );
}
