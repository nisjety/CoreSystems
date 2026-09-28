"use client";

import { useId, useState, type CSSProperties } from "react";
import {
  ArrowRight,
  BookOpen,
  Calendar,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleCheck,
  Code2,
  Copy,
  Download,
  FileText,
  Globe2,
  Link2,
  ListChecks,
  MessageSquare,
  PanelLeftClose,
  PanelRightClose,
  PanelRightOpen,
  Play,
  Plus,
  RotateCcw,
  Search,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  SquarePen,
  X,
} from "lucide-react";
import { VerevonMark } from "@/components/home/sections/VerevonMark";
import { initialEmail, planSteps, scenarios } from "./demo-data";
import s from "./product-demo.module.css";

type DemoState = {
  ready: boolean;
  message: string;
  followup: string;
  reviewed: boolean;
  checked: boolean[];
  tested: boolean;
  email: string;
  time: string;
  frequency: string;
};
type OutputTab = "result" | "sources";
const newState = (): DemoState => ({
  ready: true,
  message: "",
  followup: "",
  reviewed: false,
  checked: [false, false, false, false],
  tested: false,
  email: initialEmail,
  time: "08:30",
  frequency: "Hverdager",
});
const navigation = [
  { icon: MessageSquare, label: "Finn grunnlaget" },
  { icon: ListChecks, label: "Lag en plan" },
  { icon: Code2, label: "Undersøk blokkeringen" },
  { icon: Calendar, label: "Følg opp" },
];
const demoViews = [
  { label: "Finn grunnlaget", title: "Hele arbeidet, samlet på ett sted." },
  { label: "Lag en plan", title: scenarios[1].title },
  { label: "Undersøk blokkeringen", title: scenarios[2].title },
  { label: "Følg opp", title: scenarios[3].title },
  { label: "Hent innhold", title: "Hent innhold. Bygg kunnskap." },
];
const taskActivity = [
  ["Kilder sammenstilt", "Avhengigheter prioritert", "Planutkast klart"],
  ["Eksportlogg lest", "Skjema sammenlignet", "Endring klar for test"],
  ["Status samlet", "Kundeutkast skrevet", "Venter på gjennomgang"],
] as const;

function DashboardHome({ select }: { select: (index: number) => void }) {
  const [focus, setFocus] = useState<"leveranse" | "team" | "kilder">("leveranse");
  return (
    <div className={s.dashboard}>
      <aside className={s.dashboardRail} aria-label="Verevon navigasjon">
        <VerevonMark className={s.mark} />
        <button aria-current="page" aria-label="Oversikt"><ListChecks size={18} /></button>
        <button aria-label="Åpne plan-demo" onClick={() => select(1)}><MessageSquare size={18} /></button>
        <button aria-label="Åpne oppfølging" onClick={() => select(3)}><Calendar size={18} /></button>
        <button aria-label="Vis kilder" onClick={() => setFocus("kilder")}><BookOpen size={18} /></button>
      </aside>
      <div className={s.dashboardMain}>
        <div className={s.dashboardHeading}>
          <span className={s.eyebrow}>OVERSIKT / VELMORA DEMO</span>
          <h2>God morgen, teamet.</h2>
          <p>Prosjekt Orion trenger én avklaring før arbeidet kan gå videre.</p>
        </div>
        <div className={s.dashboardSwitch} role="group" aria-label="Velg oversikt">
          <button aria-pressed={focus === "leveranse"} onClick={() => setFocus("leveranse")}>Leveranse</button>
          <button aria-pressed={focus === "team"} onClick={() => setFocus("team")}>Team</button>
          <button aria-pressed={focus === "kilder"} onClick={() => setFocus("kilder")}>Kilder</button>
        </div>
        <div className={s.dashboardGrid}>
          <section className={s.dashboardFeature}>
            <span className={s.eyebrow}>{focus === "leveranse" ? "PROSJEKT ORION" : focus === "team" ? "TEAMETS ARBEID" : "KILDEGRUNNLAG"}</span>
            <h3>{focus === "leveranse" ? "Fristen er 14. oktober. Eksporten venter." : focus === "team" ? "Tre roller. Ett neste steg." : "Tre kilder. Én synlig konflikt."}</h3>
            <p>{focus === "leveranse" ? "Status er grønn, men eksportskjemaet mangler godkjenning. Verevon viser avhengigheten fra avtale til brukertest." : focus === "team" ? "Produktansvarlig godkjenner skjemaet. Teknisk ansvarlig tester eksporten. Kundeteamet står klart til brukertest." : "Kundeavtalen setter fristen. Møtenotatet viser avhengigheten. Teknisk status viser at eksporten er blokkert."}</p>
            <button onClick={() => select(1)}>Lag en plan <ChevronRight size={16} /></button>
          </section>
          <section className={s.dashboardCard}>
            <span className={s.eyebrow}>KILDER</span>
            <strong>3</strong>
            <p>Kundeavtale, møtenotat og teknisk status er samlet.</p>
            <button onClick={() => setFocus("kilder")}>Se kildene <ChevronRight size={15} /></button>
          </section>
          <section className={s.dashboardCard}>
            <span className={s.eyebrow}>NESTE BESLUTNING</span>
            <strong>01</strong>
            <p>Godkjenn skjema før eksport og brukertest.</p>
            <button onClick={() => select(1)}>Se planen <ChevronRight size={15} /></button>
          </section>
          <section className={s.dashboardCard}>
            <span className={s.eyebrow}>OPPFØLGING</span>
            <strong>08:30</strong>
            <p>Et forslag til intern statuskontroll venter på gjennomgang.</p>
            <button onClick={() => select(3)}>Se utkastet <ChevronRight size={15} /></button>
          </section>
        </div>
      </div>
    </div>
  );
}

function ComposerStart() {
  const [mode, setMode] = useState<"lenke" | "crawl" | "produkter">("crawl");
  const [address, setAddress] = useState("");
  const [notice, setNotice] = useState("");
  return (
    <div className={s.startView}>
      <div className={s.startContent}>
        <span className={s.eyebrow}>05 / CRAWL</span>
        <h2>Hent innhold.<br />Bygg kunnskap.</h2>
        <div className={s.crawlControls} role="group" aria-label="Velg innholdstype">
          <button aria-pressed={mode === "lenke"} onClick={() => { setMode("lenke"); setNotice(""); }}><Link2 size={14} /> Lenke</button>
          <button aria-pressed={mode === "crawl"} onClick={() => { setMode("crawl"); setNotice(""); }}><Globe2 size={14} /> Crawl</button>
          <button aria-pressed={mode === "produkter"} onClick={() => { setMode("produkter"); setNotice(""); }}><ShoppingBag size={14} /> Produkter</button>
        </div>
        <form className={s.startComposer} onSubmit={(event) => { event.preventDefault(); setNotice(address.trim() ? "Dette er en lokal visning. Ingen nettside er hentet eller lagret." : "Lim inn en nettadresse for å prøve feltet."); }}>
          <Link2 size={17} />
          <input aria-label="Nettadresse for Verevon" type="url" placeholder={mode === "produkter" ? "Lim inn en produktside …" : mode === "lenke" ? "Lim inn en lenke …" : "Lim inn en nettside for å crawle."} value={address} onChange={(event) => setAddress(event.target.value)} />
          <button aria-label="Prøv innhenting i demoen" type="submit"><ArrowRight size={17} /></button>
        </form>
        <div className={s.crawlModes} aria-hidden="true"><span>CHAT</span><span className={s.crawlActive}>CRAWL</span><span>SØK</span></div>
        <p>Hent nettsider og eksterne kilder inn i Verevon, behold kildehenvisningene og gjør innholdet klart for søk og agenter.</p>
        <div className={s.startNotice} role="status">{notice}</div>
      </div>
    </div>
  );
}

function TaskResult({
  index,
  state,
  update,
  openSource,
}: {
  index: number;
  state: DemoState;
  update: (patch: Partial<DemoState>) => void;
  openSource: (source: number) => void;
}) {
  if (index === 0)
    return (
      <>
        <div className={s.documentTitle}>
          <span className={s.eyebrow}>GRUNNLAG / ORION</span>
          <h2>Leveransen trenger en beslutning i dag.</h2>
          <p>Tre kilder. Én avhengighet som må avklares.</p>
        </div>
        <div className={s.metrics}>
          <div>
            <span>Avtalt levering</span>
            <strong>14. oktober</strong>
          </div>
          <div>
            <span>Grunnlag</span>
            <strong>3 kilder</strong>
          </div>
        </div>
        <section className={s.conflict}>
          <span className={s.status}>Krever avklaring</span>
          <h3>Grønn status. Blokkert eksport.</h3>
          <p>
            Statusrapporten sier grønn, men tidsplanen avhenger av et skjema som
            ennå ikke er godkjent.
          </p>
          <button onClick={() => openSource(2)}>
            Se statusrapporten <ChevronRight size={14} />
          </button>
        </section>
        <h3 className={s.sectionTitle}>Hva må skje først?</h3>
        <ol className={s.dependencies}>
          {[
            "Godkjenn skjema",
            "Klargjør eksport · 2 arbeidsdager",
            "Gjennomfør brukertest",
          ].map((text, i) => (
            <li key={text}>
              <span>{i + 1}</span>
              <div>
                {text}
                <small>
                  {i === 0
                    ? "Beslutningen som åpner neste steg"
                    : "Avhenger av forrige steg"}
                </small>
              </div>
            </li>
          ))}
        </ol>
        <button className={s.sourceLink} onClick={() => openSource(0)}>
          <FileText size={14} /> Se hva kundeavtalen sier{" "}
          <ChevronRight size={14} />
        </button>
      </>
    );
  if (index === 1)
    return (
      <>
        <div className={s.documentTitle}>
          <span className={s.eyebrow}>PLANUTKAST / ORION</span>
          <h2>Fra avklaring til leveranse.</h2>
          <p>
            Ansvar, rekkefølge og ferdigkriterier. Ingen oppgaver er opprettet.
          </p>
        </div>
        <div className={s.progressLabel}>
          <span>Gjennomgåtte steg</span>
          <strong>{state.checked.filter(Boolean).length} / 4</strong>
        </div>
        <div className={s.progress}>
          <div
            style={{ width: `${state.checked.filter(Boolean).length * 25}%` }}
          />
        </div>
        <div className={s.plan}>
          {planSteps.map((step, i) => (
            <section key={step.title} data-checked={state.checked[i]}>
              <div className={s.planHeading}>
                <input
                  type="checkbox"
                  aria-label={`Gjennomgått: ${step.title}`}
                  checked={state.checked[i]}
                  onChange={(e) =>
                    update({
                      checked: state.checked.map((value, n) =>
                        n === i ? e.target.checked : value,
                      ),
                      reviewed: false,
                    })
                  }
                />
                <h3>{step.title}</h3>
                <span>0{i + 1}</span>
              </div>
              <div className={s.stepMeta}>
                <span>{step.owner}</span>
                <span>{step.deadline}</span>
              </div>
              <details>
                <summary>
                  Avhengighet og ferdigkriterium <ChevronDown size={13} />
                </summary>
                <p>
                  {step.dependency}. {step.criterion}
                </p>
              </details>
            </section>
          ))}
        </div>
        <Review
          state={state}
          update={update}
          disabled={!state.checked.every(Boolean)}
          hint="Gå gjennom alle fire steg før du markerer planen som gjennomgått."
        />
      </>
    );
  if (index === 2)
    return (
      <>
        <div className={s.documentTitle}>
          <span className={s.eyebrow}>UNDERSØKELSE / TESTMILJØ</span>
          <h2>Et manglende felt stopper eksporten.</h2>
          <p>Forslaget gjelder kun testmiljø. Ingen kundedata endres.</p>
        </div>
        <div className={s.log}>
          <div>
            <Code2 size={14} /> export-184.log <span>09:41:03</span>
          </div>
          <code>
            ValidationError
            <br />
            Required field: customer_id
            <br />
            <span>Export stopped · 0 rows written</span>
          </code>
        </div>
        <button className={s.sourceLink} onClick={() => openSource(1)}>
          <BookOpen size={14} /> Sammenlign med godkjent skjema{" "}
          <ChevronRight size={14} />
        </button>
        <h3 className={s.sectionTitle}>Foreslått endring</h3>
        <div className={s.diff}>
          <div>
            customer-mapping.ts <span>+1 linje</span>
          </div>
          <pre>
            <code>
              {
                '  export const mapping = {\n    name: "name",\n    email: "email",\n'
              }
              <span>{'+   customer_id: "id",\n'}</span>
              {"  };"}
            </code>
          </pre>
        </div>
        <div className={s.testResult} aria-live="polite">
          {state.tested ? (
            <>
              <CircleCheck size={19} />
              <div>
                <strong>Simulert test bestått</strong>
                <p>3 felt validert · ingen data skrevet.</p>
              </div>
            </>
          ) : (
            <>
              <Play size={19} />
              <div>
                <strong>Verifiser før du tar neste steg</strong>
                <p>Testen bruker forhåndslagde eksempeldata.</p>
              </div>
            </>
          )}
        </div>
        <button
          className={s.secondary}
          onClick={() => update({ tested: !state.tested, reviewed: false })}
        >
          {state.tested ? <RotateCcw size={14} /> : <Play size={14} />}{" "}
          {state.tested ? "Tilbakestill testen" : "Kjør simulert test"}
        </button>
        <Review
          state={state}
          update={update}
          disabled={!state.tested}
          hint="Kjør den simulerte testen før du markerer endringen som gjennomgått."
        />
      </>
    );
  return (
    <>
      <div className={s.documentTitle}>
        <span className={s.eyebrow}>OPPFØLGING / ORION</span>
        <h2>En tydelig oppdatering. Ingen utsending.</h2>
        <p>Rediger utkastet og velg hvordan teamet skal følge opp.</p>
      </div>
      <div className={s.email}>
        <div>
          <SquarePen size={14} /> Kundeoppdatering <span>Utkast</span>
        </div>
        <label>
          Til
          <input value="Kundeteamet · Orion" readOnly />
        </label>
        <label>
          Emne
          <input value="Status for kundeportalen" readOnly />
        </label>
        <textarea
          aria-label="Kundeoppdatering"
          value={state.email}
          onChange={(e) => update({ email: e.target.value, reviewed: false })}
        />
      </div>
      <section className={s.schedule}>
        <h3>
          <Calendar size={16} /> Foreslått oppfølging
        </h3>
        <div>
          <label>
            Frekvens
            <select
              value={state.frequency}
              onChange={(e) =>
                update({ frequency: e.target.value, reviewed: false })
              }
            >
              <option>Hverdager</option>
              <option>Hver mandag</option>
              <option>Hver dag</option>
            </select>
          </label>
          <label>
            Tidspunkt
            <input
              type="time"
              value={state.time}
              onInput={(e) =>
                update({ time: e.currentTarget.value, reviewed: false })
              }
            />
          </label>
        </div>
        <p>
          Sjekk frist og blokkeringer. Varsle prosjektleder internt ved
          endringer. Rutinen er ikke aktivert.
        </p>
      </section>
      <Review
        state={state}
        update={update}
        disabled={!state.email.trim() || !state.time}
        hint="Fyll inn et utkast og et tidspunkt før gjennomgang."
      />
    </>
  );
}

function Review({
  state,
  update,
  disabled,
  hint,
}: {
  state: DemoState;
  update: (patch: Partial<DemoState>) => void;
  disabled: boolean;
  hint: string;
}) {
  return (
    <section className={s.review}>
      <ShieldCheck size={19} />
      <div>
        <strong>
          {state.reviewed ? "Gjennomgått i demoen" : "Du bestemmer neste steg"}
        </strong>
        <p>
          {state.reviewed
            ? "Kun lokal demostatus. Ingenting er sendt, opprettet eller endret i et system."
            : disabled
              ? hint
              : "Grunnlaget er klart for din gjennomgang. Dette er fortsatt et forslag."}
        </p>
        <button
          className={s.primary}
          disabled={disabled && !state.reviewed}
          onClick={() => update({ reviewed: !state.reviewed })}
        >
          {state.reviewed ? <RotateCcw size={14} /> : <Check size={14} />}{" "}
          {state.reviewed ? "Angre gjennomgang" : "Marker som gjennomgått"}
        </button>
      </div>
    </section>
  );
}

export function ProductDemo({ embeddedStep }: { embeddedStep?: 1 | 2 | 3 } = {}) {
  const embedded = embeddedStep !== undefined;
  const outputId = useId();
  const Root = embedded ? "div" : "main";
  const [selected, setActive] = useState(0);
  const active = embeddedStep ?? selected;
  const [states, setStates] = useState<DemoState[]>(() =>
    scenarios.map(newState),
  );
  const [tab, setTab] = useState<OutputTab>("result");
  const [source, setSource] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [history, setHistory] = useState(!embedded);
  const [output, setOutput] = useState(true);
  const [wide, setWide] = useState(false);
  const [notice, setNotice] = useState("");
  const item = scenarios[Math.min(active, 3)];
  const state = states[Math.min(active, 3)];
  const update = (patch: Partial<DemoState>) =>
    setStates((previous) =>
      previous.map((value, i) =>
        i === active ? { ...value, ...patch } : value,
      ),
    );
  const select = (index: number) => {
    if (embedded) return;
    setActive(index);
    setSource(null);
    setTab("result");
    setQuery("");
    setOutput(true);
    setNotice("");
  };
  const resetTask = (empty = false) => {
    setStates((previous) =>
      previous.map((value, i) =>
        i === active ? { ...newState(), ready: !empty } : value,
      ),
    );
    setTab("result");
    setSource(null);
    setQuery("");
    setOutput(true);
    setNotice(
      empty
        ? "Eksemplet er nullstilt. Kjør oppgaven for å vise resultatet igjen."
        : "Oppgaven er tilbake til det opprinnelige demoutkastet.",
    );
  };
  const openSource = (index: number) => {
    setSource(index);
    setTab("sources");
    setOutput(true);
  };
  const run = () => {
    update({ ready: true, followup: state.message.trim(), message: "" });
    setOutput(true);
    setTab("result");
    setSource(null);
    setNotice(
      "Forberedt demosvar vist. Ingen AI-kall eller eksterne handlinger er utført.",
    );
  };
  const artifactText = () =>
    `${item.artifact}\n\n${active === 3 ? `${state.email}\n\nOppfølging: ${state.frequency} kl. ${state.time}` : active === 1 ? planSteps.map((step, i) => `${state.checked[i] ? "[x]" : "[ ]"} ${step.title} · ${step.owner} · ${step.deadline}\n${step.criterion}`).join("\n\n") : `${item.answer}\n\n${item.sources.map((value) => `${value.name}\n${value.excerpt}`).join("\n\n")}${active === 2 ? `\n\nSimulert test: ${state.tested ? "Bestått" : "Ikke kjørt"}` : ""}`}\n\nLokal demo · eksempeldata · ${state.reviewed ? "Gjennomgått" : "Utkast"}`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(artifactText());
      setNotice("Resultatet er kopiert.");
    } catch {
      setNotice("Nettleseren tillot ikke kopiering. Bruk Last ned i stedet.");
    }
  };
  const download = () => {
    const url = URL.createObjectURL(
      new Blob([artifactText()], { type: "text/plain;charset=utf-8" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = `orion-${active + 1}-demo.txt`;
    link.click();
    URL.revokeObjectURL(url);
    setNotice("Demoutkastet er lastet ned.");
  };
  const matchingSources = item.sources
    .map((entry, index) => ({ ...entry, index }))
    .filter((entry) =>
      `${entry.name} ${entry.excerpt}`
        .toLocaleLowerCase("nb")
        .includes(query.toLocaleLowerCase("nb")),
    );

  return (
    <Root className={`${s.page} ${embedded ? s.embedded : ""}`}>
      {!embedded && <>
      <header className={s.intro}>
        <span className={s.eyebrow}>VEREVON / INTERAKTIV PRODUKTDEMO</span>
        <h1>Se hele arbeidet, fra spørsmål til neste steg.</h1>
        <p>
          Prøv Verevon med prosjekt Orion. Fiktiv organisasjon, eksempeldata og
          lokale handlinger.
        </p>
      </header>
      <nav className={s.scenarios} aria-label="Velg produkteksempel">
        {demoViews.map((entry, i) => (
          <button
            key={entry.label}
            aria-pressed={active === i}
            disabled={embedded && i !== active}
                    onClick={() => select(i)}
          >
            <span>0{i + 1}</span>
            {entry.label}
          </button>
        ))}
      </nav>
      </>}
      <section className={s.scene} aria-label="Verevon produktdemo">
        <div className={s.sceneHeader} aria-hidden="true">
          <span>VEREVON / PRODUKTET I BRUK</span>
          <span>0{active + 1} — 05</span>
        </div>
        <div
          className={s.app}
          style={{ "--output-width": wide ? "48%" : "40%" } as CSSProperties}
        >
          <header className={s.topbar}>
            <div className={s.brand}>
              <VerevonMark className={s.mark} />
            </div>
            <button
              className={s.iconButton}
              hidden={embedded}
              aria-label="Forrige oppgave"
              onClick={() => select((active + 4) % 5)}
            >
              <ChevronLeft size={16} />
            </button>
            <button
              className={s.iconButton}
              hidden={embedded}
              aria-label="Neste oppgave"
              onClick={() => select((active + 1) % 5)}
            >
              <ChevronRight size={16} />
            </button>
            <span className={s.slash}>/</span>
            <strong>Verevon</strong>
            <span className={s.expert}>Expert · demo</span>
            <span className={s.slash}>/</span>
            <span className={s.chatLabel}>{active === 0 ? "Oversikt" : "Chat"}</span>
            <button
              className={s.search}
              aria-label="Søk i kunnskapsbasen"
              onClick={() => {
                if (active === 0 || active === 4) select(1);
                setTab("sources");
                setSource(null);
                setOutput(true);
              }}
            >
              <Search size={15} />
              <span>Søk i kunnskapsbasen</span>
              <BookOpen size={14} />
            </button>
            <span className={s.example}>Velmora Demo</span>
            <span className={s.avatar}>V</span>
          </header>
          <div className={s.body}>
            {active === 0 ? (
              <DashboardHome select={select} />
            ) : active === 4 ? (
              <ComposerStart />
            ) : (
              <>
            <nav className={s.rail} aria-label="Oppgaveområder">
              <button
                className={s.iconButton}
                aria-label={
                  history ? "Skjul samtalehistorikk" : "Vis samtalehistorikk"
                }
                aria-pressed={history}
                onClick={() => setHistory(!history)}
              >
                <PanelLeftClose size={17} />
              </button>
              <div>
                {navigation.map(({ icon: Icon, label }, i) => (
                  <button
                    key={label}
                    title={label}
                    aria-label={label}
                    aria-current={i === active ? "page" : undefined}
                    disabled={embedded && i !== active}
                    onClick={() => select(i)}
                  >
                    <Icon size={18} />
                  </button>
                ))}
              </div>
              <button
                className={s.iconButton}
                aria-label="Vis kilder"
                onClick={() => {
                  setTab("sources");
                  setSource(null);
                  setOutput(true);
                }}
              >
                <BookOpen size={18} />
              </button>
            </nav>
            {history && (
              <aside className={s.history}>
                <button className={s.newChat} onClick={() => resetTask(true)}>
                  <Plus size={17} /> Ny samtale
                </button>
                <span className={s.eyebrow}>PROSJEKT ORION</span>
                {scenarios.map((entry, i) => (
                  <button
                    className={s.historyItem}
                    key={entry.label}
                    aria-current={i === active ? "page" : undefined}
                    disabled={embedded && i !== active}
                    onClick={() => select(i)}
                  >
                    {entry.label}
                    <small>
                      {states[i].reviewed
                        ? "Gjennomgått"
                        : states[i].ready
                          ? "Utkast klart"
                          : "Ny samtale"}
                      <span>0{i + 1}</span>
                    </small>
                  </button>
                ))}
                <div className={s.historyBottom}>
                  <ShieldCheck size={16} />
                  <p>
                    Lokalt arbeidsområde
                    <small>Ingen ekte data eller handlinger.</small>
                  </p>
                </div>
              </aside>
            )}
            <section className={s.workspace} aria-label="Samtale">
              <header className={s.chatHead}>
                <div>
                  <strong>{item.label} · Orion</strong>
                  <small>
                    {state.ready
                      ? `${state.followup ? 3 : 2} meldinger · eksempel`
                      : "1 melding · eksempel"}
                  </small>
                </div>
                <button
                  className={s.secondary}
                  aria-expanded={output}
                  onClick={() => setOutput(!output)}
                >
                  {output ? (
                    <PanelRightClose size={14} />
                  ) : (
                    <PanelRightOpen size={14} />
                  )}{" "}
                  Resultat <ChevronDown size={13} />
                </button>
              </header>
              <div className={s.conversation} key={active}>
                <div className={s.messageLabel}>
                  Deg <span>Eksempeloppgave</span>
                </div>
                <div className={s.userMessage}>
                  {item.prompt}
                  <div>
                    <FileText size={13} /> orion-grunnlag.md
                  </div>
                </div>
                <div className={s.response}>
                  <div className={s.author}>
                    <VerevonMark className={s.mark} />
                    <strong>Verevon</strong>
                    <span>Demo</span>
                  </div>
                  {state.ready ? (
                    <>
                      <p>{item.answer}</p>
                      <ol className={s.taskActivity} aria-label="Slik arbeidet Verevon">
                        {taskActivity[active - 1].map((step, index) => (
                          <li key={step}>
                            <CircleCheck size={14} />
                            <span>{step}</span>
                            <small>0{index + 1}</small>
                          </li>
                        ))}
                      </ol>
                      <button
                        className={s.artifact}
                        onClick={() => {
                          setTab("result");
                          setSource(null);
                          setOutput(true);
                        }}
                      >
                        <FileText size={15} />
                        {item.artifact}
                        <ChevronRight size={14} />
                      </button>
                      <div className={s.citations}>
                        {item.sources.map((entry, i) => (
                          <button
                            key={entry.name}
                            onClick={() => openSource(i)}
                            title={entry.name}
                          >
                            {i + 1}
                            <span>{entry.name.split(" · ")[0]}</span>
                          </button>
                        ))}
                      </div>
                      {state.followup && (
                        <div className={s.followup}>
                          <span>Ditt oppfølgingsspørsmål</span>
                          <p>{state.followup}</p>
                          <small>
                            Denne demoen viser det forberedte svaret for «
                            {item.label}», ikke et nytt AI-generert svar.
                          </small>
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      <h2>{item.title}</h2>
                      <p>Prøv oppgaven og se det forberedte resultatet.</p>
                      <button className={s.primary} onClick={run}>
                        <Sparkles size={15} /> Kjør eksemplet
                      </button>
                    </>
                  )}
                </div>
              </div>
              <div className={s.taskStatus} role="status">
                {notice || "Lokal demo · ingen AI-kall, utsending eller systemendringer."}
              </div>
            </section>
            {output && (
              <aside className={s.output} aria-label="Resultat og kilder">
                <header className={s.outputHead}>
                  <nav aria-label="Arbeidsflate">
                    <button
                      aria-controls={outputId}
                      aria-pressed={tab === "result"}
                      onClick={() => {
                        setTab("result");
                        setSource(null);
                      }}
                    >
                      <FileText size={14} /> Resultat{" "}
                      <span>{state.ready ? 1 : 0}</span>
                    </button>
                    <button
                      aria-controls={outputId}
                      aria-pressed={tab === "sources"}
                      onClick={() => {
                        setTab("sources");
                        setSource(null);
                      }}
                    >
                      <BookOpen size={14} /> Kilder <span>3</span>
                    </button>
                  </nav>
                  <button
                    className={s.iconButton}
                    aria-label={
                      wide
                        ? "Gjør resultatet smalere"
                        : "Gjør resultatet bredere"
                    }
                    onClick={() => setWide(!wide)}
                  >
                    <PanelRightOpen size={16} />
                  </button>
                  <button
                    className={s.iconButton}
                    aria-label="Lukk resultat"
                    onClick={() => setOutput(false)}
                  >
                    <X size={15} />
                  </button>
                </header>
                <div
                  id={outputId}
                  key={`${active}-${tab}`}
                  className={s.outputContent}
                >
                  {tab === "sources" ? (
                    <>
                      <label className={s.sourceSearch}>
                        <Search size={15} />
                        <input
                          aria-label="Søk i eksempelkilder"
                          placeholder="Søk i eksempelkilder …"
                          value={query}
                          onChange={(e) => {
                            setQuery(e.target.value);
                            setSource(null);
                          }}
                        />
                      </label>
                      <span className={s.eyebrow}>KUNNSKAP / ORION</span>
                      <h2 className={s.sourceTitle}>Grunnlaget bak svaret.</h2>
                      {source === null ? (
                        <div className={s.sourceList}>
                          {matchingSources.map((entry) => (
                            <button
                              key={entry.name}
                              onClick={() => setSource(entry.index)}
                            >
                              <FileText size={18} />
                              <div>
                                <strong>{entry.name}</strong>
                                <small>{entry.type} · eksempeldata</small>
                              </div>
                              <ChevronRight size={15} />
                            </button>
                          ))}
                          {matchingSources.length === 0 && (
                            <p>Ingen kilder matcher «{query}».</p>
                          )}
                        </div>
                      ) : (
                        <article className={s.sourcePreview}>
                          <button
                            className={s.sourceLink}
                            onClick={() => setSource(null)}
                          >
                            <ChevronLeft size={14} /> Alle kilder
                          </button>
                          <span className={s.status}>
                            {item.sources[source].type}
                          </span>
                          <h3>{item.sources[source].name}</h3>
                          <blockquote>
                            {item.sources[source].excerpt}
                          </blockquote>
                          <small>{item.sources[source].detail}</small>
                        </article>
                      )}
                    </>
                  ) : state.ready ? (
                    <>
                      <div className={s.artifactHeader}>
                        <FileText size={17} />
                        <div>
                          <strong>{item.artifact}</strong>
                          <small>{item.kind} · eksempeldata</small>
                        </div>
                        <span className={s.status}>
                          {state.reviewed ? "Gjennomgått" : "Utkast"}
                        </span>
                      </div>
                      <div className={s.documentTools}>
                        <button onClick={copy}>
                          <Copy size={13} /> Kopier
                        </button>
                        <button onClick={download}>
                          <Download size={13} /> Last ned
                        </button>
                        <button
                          aria-label="Nullstill denne oppgaven"
                          onClick={() => resetTask()}
                        >
                          <RotateCcw size={13} />
                        </button>
                      </div>
                      <TaskResult
                        index={active}
                        state={state}
                        update={update}
                        openSource={openSource}
                      />
                    </>
                  ) : (
                    <div className={s.empty}>
                      <FileText size={30} />
                      <h2>Plass til det du får gjort.</h2>
                      <p>
                        Kjør oppgaven for å se resultat, kilder og neste steg.
                      </p>
                      <button className={s.primary} onClick={run}>
                        <Play size={14} /> Kjør eksemplet
                      </button>
                    </div>
                  )}
                </div>
              </aside>
            )}
              </>
            )}
          </div>
        </div>
        <div className={s.sceneFooter} aria-hidden="true">
          <span>VELMORA DEMO · PROSJEKT ORION</span>
          <span>KILDER → ARBEID → BESLUTNING</span>
        </div>
      </section>
      {!embedded && <footer className={s.reviewFooter}>
        <p>
          <span>0{active + 1} / 05</span>
          {demoViews[active].title}
        </p>
        <button onClick={() => select((active + 1) % 5)}>
          {active === 4 ? "Tilbake til oversikten" : "Neste visning"} <ChevronRight size={16} />
        </button>
      </footer>}
    </Root>
  );
}
