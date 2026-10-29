import type { ServiceDelivery } from "./service-delivery-data";
import type { ServiceSeo } from "./service-seo";

// English version of service-delivery-data.ts. Keys and hrefs are the
// Norwegian route keys; links are localized when rendered.
export const serviceDeliveriesEn: Record<
  ServiceSeo["canonical"],
  ServiceDelivery
> = {
  "/tjenester/web-losninger": {
    deliverables: [
      {
        title: "Structure and content",
        description:
          "Audiences, page types, navigation and priority user journeys are clarified before design. We agree on who writes, approves and maintains the content.",
      },
      {
        title: "Design and development",
        description:
          "Responsive page templates, reusable components and a publishing setup that suits your editors. WordPress or a custom solution is chosen based on content, integrations and long-term management.",
      },
      {
        title: "Launch and handover",
        description:
          "A plan for the domain, existing URLs, redirects and publishing. Documentation and editor training can be included, so your team knows how to update the website.",
      },
    ],
    collaboration:
      "We can deliver the website as a defined project or work alongside your designers, editors and developers. Access, decision-making and sign-off of pages are agreed at the start.",
    quality:
      "Before publishing, we review the agreed user journeys, forms, keyboard navigation, mobile layout and performance. Known issues and any outstanding items are collected, so the launch decision rests on a visible checklist.",
    continuation:
      "After handover, we can agree on maintenance and further development. Responsibility for hosting, updates, backups and incident handling is set out in the agreement, so it is clear who looks after the website.",
    related: [
      {
        before: "If the website should make you easier to find, we plan ",
        label: "search engine optimisation and content",
        href: "/tjenester/organisk-trafikk",
        after: " together with structure and publishing.",
      },
      {
        before:
          "If the technology choice or scope is still open, you can start with ",
        label: "digital advisory",
        href: "/tjenester/radgivning-og-konsulentbistand",
        after: " before development begins.",
      },
    ],
  },
  "/tjenester/nettbutikk": {
    deliverables: [
      {
        title: "A clear buying journey",
        description:
          "Categories, search, product information, basket and checkout are designed around how your customers shop. We clarify variants, languages and any different customer types early on.",
      },
      {
        title: "Store and operations",
        description:
          "Setting up the platform, product data, payment and shipping options based on the agreed providers. Needs for order, stock and accounting flows are mapped before we start on integrations.",
      },
      {
        title: "A safe migration",
        description:
          "When changing platforms, we plan for products, URLs and redirects. Test imports and reconciliation make it possible to spot gaps before the new store takes over.",
      },
    ],
    collaboration:
      "We work with the people who own product data, customer service, logistics and finance. Where you have an in-house development team or a platform partner, we divide responsibility for the store, data and integrations before work starts.",
    quality:
      "We test the agreed purchase flows, including payment, abandoned purchases, order confirmation and relevant shipping options. Test orders are reconciled against the systems that receive them, and the buying journey is checked on mobile.",
    continuation:
      "We can follow up technical updates and prioritise improvements based on real purchase flows. Ongoing product work, support, third-party costs and further development are agreed as separate areas of responsibility.",
    related: [
      {
        before:
          "When orders and stock need to connect with the rest of the business, ",
        label: "system integrations",
        href: "/tjenester/integrasjoner",
        after: " are part of the planning.",
      },
      {
        before: "Categories and product pages also lay the groundwork for ",
        label: "organic visibility",
        href: "/tjenester/organisk-trafikk",
        after: ", while advertising can support specific campaigns.",
      },
    ],
  },
  "/tjenester/organisk-trafikk": {
    deliverables: [
      {
        title: "Audit and prioritisation",
        description:
          "We review search intent, existing pages and technical obstacles. You get a prioritised list of actions, explaining which customer needs each page should answer.",
      },
      {
        title: "Technical work and content",
        description:
          "The work can cover indexing, internal links, metadata, redirects and content structure. Your subject-matter experts contribute the experience and documentation that make the content distinctive.",
      },
      {
        title: "Measuring relevant traffic",
        description:
          "We agree on which searches, pages and enquiries matter. Search Console and suitable analytics are used to follow progress and to separate visibility from visits that are actually relevant.",
      },
    ],
    collaboration:
      "SEO work is shared between marketing, subject-matter experts and the development team. We can deliver analyses and content briefs, implement agreed actions or support your team in carrying them out.",
    quality:
      "We check that important pages can be visited and indexed, that the content answers the search, and that links and structured data match the page. Traffic and enquiries are assessed over time; rankings or specific results cannot be guaranteed.",
    continuation:
      "After the first round of improvements, we prioritise based on data and changes in what you offer. Relevant questions in Google Trends can inform the content plan, together with your own customer conversations and search data, without letting short-lived trends steer the whole website.",
    related: [
      {
        before:
          "When technical limitations hold the improvements back, we connect the SEO work to ",
        label: "web development",
        href: "/tjenester/web-losninger",
        after: " and a concrete plan for the changes.",
      },
      {
        before: "If you need visibility while organic content is being built, ",
        label: "Google Ads and Meta Ads",
        href: "/tjenester/google-og-meta-ads",
        after: " can complement the work with agreed budgets and metrics.",
      },
    ],
  },
  "/tjenester/integrasjoner": {
    deliverables: [
      {
        title: "System and data map",
        description:
          "We describe which systems are the source of which data, what should be moved and when. Access to APIs, test environments and vendor terms is clarified before implementation.",
      },
      {
        title: "A defined data flow",
        description:
          "The integration is built for agreed events and data fields, with validation and error handling. We agree on how duplicates, changes and temporary outages should be handled.",
      },
      {
        title: "Documentation and ownership",
        description:
          "You get a description of the integration and of how errors can be detected and followed up. Owners of access, data quality and each part of operations are named in the delivery agreement.",
      },
    ],
    collaboration:
      "We work with the system owners, your IT team and any external vendors. Together we clarify what information the integration may process and who can approve changes to the data flow.",
    quality:
      "Testing covers correct transfer, invalid data and outages in a connected system. We reconcile selected records between source and destination and agree on the criteria for putting the integration into production.",
    continuation:
      "APIs and systems change. Maintenance can therefore include following up vendor changes, error alerts and new needs. Scope, response times and costs are agreed separately from the initial delivery.",
    related: [
      {
        before: "For an ",
        label: "online store",
        href: "/tjenester/nettbutikk",
        after:
          ", a sensible first scope can be orders to accounting and stock levels back to the store.",
      },
      {
        before:
          "If the solution should also interpret information or suggest a reply, we consider ",
        label: "AI and automation",
        href: "/tjenester/ki-og-automatisering",
        after: " as a separate step with clear checkpoints.",
      },
    ],
  },
  "/tjenester/google-og-meta-ads": {
    deliverables: [
      {
        title: "Goals and campaign structure",
        description:
          "We clarify the offer, the audience, the geography and what counts as a relevant enquiry or sale. Channel and campaign format are chosen on that basis, with a budget you approve.",
      },
      {
        title: "Messaging and landing pages",
        description:
          "Ads, keywords or audiences and the landing page should fit together. We agree on who supplies images and copy, and who approves the messaging before the campaign starts.",
      },
      {
        title: "Tracking and follow-up",
        description:
          "Relevant conversions are defined, tracking is checked and a reporting rhythm is agreed. Fees, ad spend and any production are kept clearly separate.",
      },
    ],
    collaboration:
      "We work with marketing and sales on the quality of the enquiries. Account access, approval routines and ownership of ads and measurement setup are clarified at the start.",
    quality:
      "Before going live, we check links, messaging, targeting, budget and the recording of agreed conversions. We review the consent setup with you and assess results against actual sales or enquiries where data is available.",
    continuation:
      "Ongoing work can include search terms, bid adjustments and testing new messages. We explain what has changed and which observations lie behind it. Results are also affected by the offer, competition and the landing page.",
    related: [
      {
        before:
          "A campaign needs a clear next step. We can improve the landing page as part of ",
        label: "web design and web development",
        href: "/tjenester/web-losninger",
        after: ".",
      },
      {
        before: "Insight from campaigns can also feed into ",
        label: "SEO and content work",
        href: "/tjenester/organisk-trafikk",
        after: " when you want to build visibility over time.",
      },
    ],
  },
  "/tjenester/ki-og-automatisering": {
    deliverables: [
      {
        title: "One task with clear value",
        description:
          "We describe who will use the solution, what data it needs and what a good result looks like. Together we choose a defined use case and compare AI with simpler automation.",
      },
      {
        title: "A pilot with checkpoints",
        description:
          "The pilot is tried on agreed questions or tasks. Sources, access and human approval are part of the setup. Actions in other systems are scoped separately.",
      },
      {
        title: "A basis for the next decision",
        description:
          "We document test results, known limitations and which errors must be handled. Model, operating and vendor costs are made visible before you decide to expand the solution.",
      },
    ],
    collaboration:
      "The people who know the task take part, together with the IT or system owner. They help with test examples and assess the answers, while we build and document the workflow.",
    quality:
      "We test correct answers, uncertain questions and situations where the sources do not provide an answer. The solution must have an agreed route to human review. Approval and data access follow the task, not the model’s own view of what it should be allowed to do.",
    continuation:
      "After a pilot, we can agree on following up answers, sources and costs. Changes to the knowledge base or model are tested before they are put to use, and further expansion builds on the experience from the defined workflow.",
    related: [
      {
        before:
          "When information needs to be fetched from or sent to existing tools, we build on ",
        label: "system integrations",
        href: "/tjenester/integrasjoner",
        after: " with agreed access and error routines.",
      },
      {
        before: "If it is unclear where AI can add value, we start with ",
        label: "advisory and needs assessment",
        href: "/tjenester/radgivning-og-konsulentbistand",
        after: " before choosing a product or technology.",
      },
    ],
  },
  "/tjenester/radgivning-og-konsulentbistand": {
    deliverables: [
      {
        title: "Assessment and roadmap",
        description:
          "We map needs, existing solutions and dependencies. You get recommendations and prioritised next steps, with assumptions and open questions visible in the decision basis.",
      },
      {
        title: "A consultant in your team",
        description:
          "We can contribute agreed expertise in web development, integrations and practical AI. Tasks, capacity, collaboration tools and who approves the deliverables are clarified before the start.",
      },
      {
        title: "A defined project",
        description:
          "For a clearly defined task, we can take responsibility for delivery from scoping to handover. Milestones and acceptance criteria make it possible to follow progress and adjust the scope along the way.",
      },
    ],
    collaboration:
      "We adapt to your project manager, product owners and technical team. Responsibility for prioritisation, architecture, code review and decisions is agreed together, so the consulting work fits into your existing organisation.",
    quality:
      "Recommendations are justified by needs and alternatives. In development assignments, we agree on code review, relevant tests, documentation and demonstrations of finished work. Open issues are raised before they become hidden dependencies.",
    continuation:
      "The assignment ends with agreed documentation and knowledge transfer. You can take over management yourselves or agree on further support; access to code, systems and decision history is part of planning the handover.",
    related: [
      {
        before: "Once the direction is clear, we can carry out ",
        label: "web development",
        href: "/tjenester/web-losninger",
        after: " together with your team or as a separate project.",
      },
      {
        before: "If you have many system dependencies, ",
        label: "an integration assessment",
        href: "/tjenester/integrasjoner",
        after: " can be the first area of work.",
      },
      {
        before: "For new ways of working, we can scope a pilot in ",
        label: "AI and automation",
        href: "/tjenester/ki-og-automatisering",
        after: " with clear goals for the evaluation.",
      },
    ],
  },
};
