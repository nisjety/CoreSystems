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
      <span className="velion-visual__asset" aria-hidden="true" />
      <span className="velion-visual__line velion-visual__line--one" />
      <span className="velion-visual__line velion-visual__line--two" />
      <span className="velion-visual__line velion-visual__line--three" />
      <span className="velion-visual__node velion-visual__node--one" />
      <span className="velion-visual__node velion-visual__node--two" />
      <span className="velion-visual__node velion-visual__node--three" />
      <span className="velion-visual__panel velion-visual__panel--one" />
      <span className="velion-visual__panel velion-visual__panel--two" />
    </div>
  );
}
