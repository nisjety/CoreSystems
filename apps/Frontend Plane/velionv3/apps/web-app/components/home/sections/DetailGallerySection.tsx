const detailNotes = ["Monitor", "Brief", "Review", "Approve", "Execute", "Audit"];

export function DetailGallerySection() {
  return (
    <section className="velion-detail" id="details">
      <div className="velion-detail__copy">
        <span className="velion-copy-label fade-out-top" data-fade-out-top>
          02 / 06
        </span>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          The approval loop is the product.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Velion can propose work across support, sales, knowledge, and operations, but the system is built around
            human control. High-impact actions stop at review with context, rollback, and audit attached.
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
