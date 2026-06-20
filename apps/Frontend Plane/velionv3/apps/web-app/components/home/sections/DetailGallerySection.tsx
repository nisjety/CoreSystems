const detailNotes = ["Surface", "Silence", "Control", "Craft", "Space", "Flow"];

export function DetailGallerySection() {
  return (
    <section className="velion-detail" id="details">
      <div className="velion-detail__copy">
        <span className="velion-copy-label fade-out-top" data-fade-out-top>
          02 / 06
        </span>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Attention to detail
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Step into a world where design transcends luxury and evolves into a sensual experience from every
            perspective.
          </p>
        </div>
        <div className="velion-detail__ticks" aria-label="Detail sequence">
          {detailNotes.map((note) => (
            <span key={note}>{note}</span>
          ))}
        </div>
      </div>

      <div className="velion-detail__stage" role="img" aria-label="Animated detail panels">
        {detailNotes.map((note, index) => (
          <span
            className={`velion-detail__frame velion-detail__frame--${index + 1}`}
            key={note}
            style={{ "--delay": `${index * 3}s` } as React.CSSProperties}
          />
        ))}
      </div>
    </section>
  );
}
