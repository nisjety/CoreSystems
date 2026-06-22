const connectorLogos = [
  "Shopify",
  "Zendesk",
  "Gorgias",
  "Klaviyo",
  "Meta",
  "Slack",
  "Teams",
  "Gmail",
  "Brreg",
  "Visma",
  "Tripletex",
  "HubSpot",
];

function LogoTrack({ hidden = false }: { hidden?: boolean }) {
  return (
    <div aria-hidden={hidden || undefined} className="velion-logo-carousel__track">
      {connectorLogos.map((logo) => (
        <span className="velion-logo-carousel__mark" key={`${hidden ? "copy" : "main"}-${logo}`}>
          {logo}
        </span>
      ))}
    </div>
  );
}

export function BrandLogosSection() {
  return (
    <section className="velion-logo-carousel" aria-label="Connected systems carousel">
      <div className="velion-logo-carousel__viewport">
        <LogoTrack />
        <LogoTrack hidden />
      </div>
    </section>
  );
}
