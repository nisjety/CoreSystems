export type VisualPanelVariant =
  | "craft"
  | "surface"
  | "mark"
  | "icon"
  | "open"
  | "detail"
  | "award"
  | "heritage";

type VisualPanelProps = {
  className?: string;
  label: string;
  variant: VisualPanelVariant;
};

export function VisualPanel({ className = "", label, variant }: VisualPanelProps) {
  return (
    <div className={`velion-visual velion-visual--${variant} ${className}`} role="img" aria-label={label}>
      <span className="velion-visual__line velion-visual__line--one" />
      <span className="velion-visual__line velion-visual__line--two" />
      <span className="velion-visual__line velion-visual__line--three" />
    </div>
  );
}
