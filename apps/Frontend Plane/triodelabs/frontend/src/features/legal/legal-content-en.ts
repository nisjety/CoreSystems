import type { Localized } from "../../lib/i18n/locales";
import {
  cookiesDocument,
  privacyDocument,
  salesTermsDocument,
  termsDocument,
  type LegalDocument,
} from "./legal-content";

// English translations of legal-content.ts. `path` stays the Norwegian route
// key; dates follow the Norwegian originals. The Norwegian text is binding.
const notice =
  "This English version is provided for convenience. If there is any discrepancy between the English and Norwegian versions, the Norwegian version prevails.";

export const privacyDocumentEn: LegalDocument = {
  path: privacyDocument.path,
  updatedAt: privacyDocument.updatedAt,
  updatedLabel: "23 October 2026",
  notice,
  title: "Privacy policy",
  description:
    "Read how TriodeLab DA processes personal data, which rights you have, and how to contact us about privacy.",
  introduction:
    "This privacy policy explains which personal data TriodeLab DA collects when you use triodelab.no or are in contact with us, what we use it for, and which rights you have.",
  sections: [
    {
      title: "Data controller",
      paragraphs: [
        "TriodeLab DA, Sigurd Hoels vei 114, 0655 Oslo, Norway, is the data controller for the personal data described in this policy. You can reach us at post@triodelab.no or +47 988 46 460.",
      ],
    },
    {
      title: "What data we process",
      paragraphs: [
        "We process personal data that you provide to us yourself, for example through the contact form on the website, by email or by phone:",
      ],
      items: [
        "Your name and the company you represent",
        "Email address and phone number",
        "The content of your enquiry, including the type of project and any budget you state",
      ],
    },
    {
      title: "What we use the data for",
      paragraphs: [
        "The data is used to reply to your enquiry, to assess and follow up potential projects, and to administer any customer relationship and invoicing. The legal basis is Article 6(1)(b) of the General Data Protection Regulation (GDPR) (performing or entering into a contract) and Article 6(1)(f) (legitimate interest in responding to enquiries).",
        "We do not use the data for automated decision-making or profiling.",
      ],
    },
    {
      title: "The contact form on the website",
      paragraphs: [
        "When submission on the website is enabled, your name, email address, any company name, the selected service and your message are sent to post@triodelab.no via the email provider Resend. If this function is not enabled, the website shows an email link so you can send the enquiry yourself.",
        "To limit misuse of the form, we use a pseudonymised network identifier that is deleted after 10 minutes. We also store a technical delivery identifier for up to 48 hours to avoid counting the same submission twice. This storage is handled by Upstash and does not contain the message text, name, email address or raw IP address.",
        "We count received enquiries in aggregate per day, per selected service and by where on the website the form was opened. These counts do not contain contact details or message text and do not use cookies in the browser.",
      ],
    },
    {
      title: "How long we keep the data",
      paragraphs: [
        "Enquiries that do not lead to a customer relationship are normally deleted within 12 months. Data relating to a customer relationship is kept for as long as necessary to fulfil the contract, and thereafter in line with statutory retention obligations, including the Norwegian Bookkeeping Act for accounting records (5 years).",
      ],
    },
    {
      title: "Who we share data with",
      paragraphs: [
        "The website is hosted by Vercel, which may process technical data (such as IP addresses) on our behalf in order to deliver the site. We do not sell personal data, and we only share it with others where necessary to deliver a service to you, or where required by law.",
        "When submission via the contact form is enabled, we use Resend to deliver the enquiry by email and Upstash for misuse protection and aggregate counting of received enquiries, as described above.",
        "After you choose Statistics, we use Microsoft Clarity on selected public pages to understand how the website is used and improve it. Microsoft receives technical data and page-view, click, scroll, pointer, layout and performance events to create heatmaps and session recordings. See Microsoft's privacy statement for more information about its processing.",
        "The Agenci chat only loads when you choose to open it. Information you type in the chat is sent to Agenci to process the conversation. Please avoid sending sensitive personal data in the chat.",
      ],
      links: [
        {
          label: "Microsoft Privacy Statement",
          href: "https://privacy.microsoft.com/privacystatement",
        },
      ],
    },
    {
      title: "Cookies",
      paragraphs: [
        "Cookiebot displays and stores your consent choices. When visitor analytics is enabled for the site, Google Analytics 4 and Microsoft Clarity load on selected public pages only after you choose Statistics. If you decline or do not choose Statistics, neither measurement script is loaded. You can change or withdraw your choice using the cookie preferences on the Cookie policy page.",
      ],
    },
    {
      title: "Optional website analytics",
      paragraphs: [
        "The purpose is to understand use of public pages and improve the website. After consent, Google may receive page views, sessions, engagement, scrolling, browser and device details, and technical connection information. Enhanced Measurement is enabled and may also record interactions with public content, such as embedded video. Cookies with pseudonymous identifiers are used to distinguish users and sessions.",
        "Measurement is limited to selected public pages. It does not start on contact, account or legal pages, unknown routes, or URLs with query strings or fragments. Measurement stops in the document on form/chat interactions and links that could expose personal information. Do not put personal or sensitive information in URLs. We do not send contact-form or chat message text as custom Analytics events.",
        "Microsoft Clarity uses Statistics data for heatmaps and session recordings. It may collect pointer movement, clicks, scrolling, viewport size, page views and technical events for rendering and performance. The site explicitly masks page content using Clarity's data-clarity-mask attribute; masked content is not sent to Clarity. Clarity also masks input fields. Contact, legal and unknown pages are excluded, and measurement stops on sensitive interactions and navigations. Microsoft states that recordings are retained for 30 days and click/heatmap data for 9 months.",
        "This setup does not grant consent to advertising storage, ad personalization or Google Signals. Google advertising storage and Clarity advertising storage remain denied. Browser Do Not Track or Global Privacy Control signals stop measurement.",
        "The GA4 settings checked on 22 October 2026 retain event data for 2 months and user data for 14 months. The user-data period resets with new activity. These settings may not limit retention of aggregated standard reports. Cookie lifetimes are described separately in the Cookie policy.",
      ],
    },
    {
      title: "Optional first-party performance measurement",
      paragraphs: [
        "When enabled and you choose Statistics, we measure LCP (loading), INP (interaction responsiveness) and CLS (layout shifts) on selected public pages. Your browser sends the measurement, a broad viewport-width group, an approved page path, a random document identifier and an update counter to this website. The identifier lasts only for that document, is not stored in a cookie and is not used to follow people between visits.",
        "Raw measurements are retained in server-side Redis-compatible storage and our marketing platform for at most 7 UTC calendar days, including the receipt day. They power aggregate performance reports, not profiling. The measurement payload does not include query strings, fragments, referrer, DOM content, user IDs or raw IP addresses. Hosting and storage providers may process technical connection data to deliver the service. Collection requires consent, respects DNT/GPC and stops on sensitive interactions or navigation. Withdrawal stops new measurements; previously received measurements expire automatically.",
      ],
    },
    {
      title: "Your rights",
      paragraphs: [
        "Under the General Data Protection Regulation, you have the right to access, rectify or erase your personal data, as well as the right to restrict or object to the processing and, where relevant, the right to data portability. If you wish to exercise any of these rights, contact us at post@triodelab.no.",
        "You can also lodge a complaint with the Norwegian Data Protection Authority (Datatilsynet) if you believe we process personal data in breach of the regulations.",
      ],
    },
  ],
};

export const cookiesDocumentEn: LegalDocument = {
  path: cookiesDocument.path,
  updatedAt: cookiesDocument.updatedAt,
  updatedLabel: "23 October 2026",
  notice,
  title: "Cookie policy",
  description:
    "Information about cookies on triodelab.no, what they are used for, and how you can manage or delete them in your browser.",
  introduction:
    "This page explains the use of cookies and other storage mechanisms on triodelab.no. We will update the page if this use changes.",
  sections: [
    {
      title: "What is a cookie?",
      paragraphs: [
        "A cookie is a small text file that is stored in your browser when you visit a website. It can be used, among other things, to remember settings, measure traffic or show relevant ads.",
      ],
    },
    {
      title: "Cookies we use today",
      paragraphs: [
        "Cookiebot displays the consent banner and stores your choice in a necessary cookie. When visitor analytics is enabled, GA4 and Clarity load only on selected public pages after you choose Statistics. Next.js and Vercel deliver the site. Misuse protection and aggregate counting of contact enquiries take place on the server and do not use browser cookies.",
      ],
    },
    {
      title: "Optional analytics cookies",
      paragraphs: [
        "After Statistics consent, GA4 may use _ga to distinguish users and _ga_<container-id> to maintain session state. Google's nominal default lifetime is 2 years; browsers may limit it, and subsequent visits may renew expiry. Clarity may use _clck to remember a pseudonymous user ID and _clsk to connect page views in a session recording. Clarity may also set Microsoft cookies for technical operation. These browser-cookie lifetimes are different from the providers' report and recording retention periods.",
        "You can withdraw consent by opening cookie preferences and turning off Statistics. The integration stops measurement, attempts to clear its first-party Google and Clarity cookies, then reloads the page. This does not automatically delete data already received by Google or Microsoft. You can also clear cookies in your browser.",
        "This integration does not enable advertising features. Clarity uses masked site data for heatmaps and recordings; Microsoft states that recordings are retained for 30 days and click/heatmap data for 9 months. See our Privacy policy and Microsoft's privacy statement for more information.",
      ],
      links: [
        {
          label: "Microsoft Privacy Statement",
          href: "https://privacy.microsoft.com/privacystatement",
        },
      ],
    },
    {
      title: "Optional cookieless first-party performance measurement",
      paragraphs: [
        "After Statistics consent, our own performance measurement may send LCP, INP and CLS to this website's server. This feature uses no cookies or persistent browser storage. A random document identifier is used only to deduplicate measurement updates. See our Privacy policy for the data fields, consent choices and maximum retention of 7 UTC calendar days.",
      ],
    },
    {
      title: "Optional chat",
      paragraphs: [
        "The chat is provided by Agenci and only loads when you choose to open it. Your browser then connects to Agenci, and what you type in the chat is sent there to process the conversation. You can use the rest of the website and contact us by email without opening the chat.",
        "The chat provider’s use of cookies and local storage is not covered by the description of visitor tracking above. We therefore do not confirm that the chat is free of cookies or local storage.",
      ],
    },
    {
      title: "Changes to cookie use",
      paragraphs: [
        "If the purposes, providers or optional cookies change, we will update this page and the consent information before enabling the new use.",
      ],
    },
    {
      title: "Managing or deleting cookies",
      paragraphs: [
        "You can delete or block cookies at any time in your browser settings. This may affect how some websites work, but it will not prevent you from using triodelab.no.",
      ],
    },
    {
      title: "Questions",
      paragraphs: [
        "If you have questions about cookies or privacy in general, see our privacy policy or contact us at post@triodelab.no.",
      ],
    },
  ],
};

export const termsDocumentEn: LegalDocument = {
  path: termsDocument.path,
  updatedAt: termsDocument.updatedAt,
  updatedLabel: "24 September 2026",
  notice,
  title: "Terms and conditions",
  description:
    "General terms for using triodelab.no and for agreements with TriodeLab DA. Read about services, liability, rights and contact details.",
  introduction:
    "These terms apply to the use of triodelab.no and to agreements you enter into with TriodeLab DA. By using the website or ordering a service from us, you accept the terms below.",
  sections: [
    {
      title: "About the company",
      paragraphs: [
        "TriodeLab DA (“TriodeLab”, “we”, “us”) is a Norwegian consultancy with its office address at Sigurd Hoels vei 114, 0655 Oslo, Norway. We deliver web solutions, online stores, integrations and digital marketing to business customers.",
        "If you have questions about these terms, you can reach us at post@triodelab.no or +47 988 46 460.",
      ],
    },
    {
      title: "Use of the website",
      paragraphs: [
        "The content on triodelab.no is owned by TriodeLab or our licensors and is protected by the Norwegian Copyright Act. You may read and share content from the site, but you may not copy, reuse or distribute it commercially without our written consent.",
        "The website must not be used for unlawful purposes, to spread malicious code, or in a way that disrupts the operation of the site for other visitors.",
      ],
    },
    {
      title: "Service agreements",
      paragraphs: [
        "Specific deliverables, prices and deadlines are agreed separately with each customer, normally in a dedicated quote or a signed agreement. Where such an agreement exists, it takes precedence over these general terms in the event of conflict.",
        "For purchases of services not governed by a separate agreement, our terms of sale apply; see “Terms of sale”.",
      ],
    },
    {
      title: "Liability",
      paragraphs: [
        "We do our best to ensure that the information on the website is correct and up to date, but we cannot guarantee that it is complete or error-free at all times.",
        "TriodeLab is not liable for indirect losses resulting from the use of the website, unless otherwise required by mandatory law or a separate agreement between the parties.",
      ],
    },
    {
      title: "Links to third parties",
      paragraphs: [
        "The website may contain links to websites we do not control, including projects and platforms we have built for customers. We are not responsible for the content of those sites.",
      ],
    },
    {
      title: "Changes",
      paragraphs: [
        "We may update these terms when needed, for example when our services or legal requirements change. The date at the top of the page shows when the terms were last updated. In the event of material changes to an ongoing customer agreement, we will notify you directly.",
      ],
    },
    {
      title: "Governing law and disputes",
      paragraphs: [
        "These terms are governed by Norwegian law. Any disputes shall first be sought resolved amicably. If this does not succeed, the dispute shall be brought before the ordinary courts of TriodeLab’s legal venue in Norway.",
      ],
    },
  ],
};

export const salesTermsDocumentEn: LegalDocument = {
  path: salesTermsDocument.path,
  updatedAt: salesTermsDocument.updatedAt,
  updatedLabel: "24 September 2026",
  notice,
  title: "Terms of sale",
  description:
    "Terms of sale for consulting and development services from TriodeLab DA: ordering, payment, deliveries, changes, complaints and rights.",
  introduction:
    "These terms of sale apply when you purchase consulting and development services from TriodeLab DA. They apply together with any quote, project agreement or order confirmation you have received from us.",
  sections: [
    {
      title: "Who the terms apply to",
      paragraphs: [
        "We deliver services to businesses (B2B). The Norwegian Consumer Purchases Act and Cancellation Act, which apply to the sale of goods and services to private individuals, therefore do not normally apply to our deliveries.",
      ],
    },
    {
      title: "Quotes and orders",
      paragraphs: [
        "An agreement is formed when you accept a quote from us in writing, for example by signing, by email or by confirming in a project tool. Quotes are normally valid for 30 days unless otherwise stated.",
        "The scope, deliverables, schedule and price for each project are set out in the quote or agreement, and may differ from what is described in general terms on the website.",
      ],
    },
    {
      title: "Price and payment",
      paragraphs: [
        "Prices are stated in Norwegian kroner (NOK) and, unless otherwise agreed, exclude value added tax. Payment terms are stated on the invoice, normally 14 days.",
        "For larger projects, we may agree on instalments, for example at the start, during the project and on delivery. Late payment interest accrues in accordance with the Norwegian Late Payment Interest Act.",
      ],
    },
    {
      title: "Delivery and customer involvement",
      paragraphs: [
        "We deliver the services with normal professional care and within the agreed time frames. Progress depends on you providing the necessary information, feedback and decisions along the way.",
        "Delays caused by a lack of involvement from the customer, or by circumstances beyond our control, do not entitle the customer to compensation or a price reduction.",
      ],
    },
    {
      title: "Changes to the project",
      paragraphs: [
        "If you want to change the scope during the project, we agree on the consequences for price and schedule before the work begins. Minor adjustments within the original scope are handled as we go.",
      ],
    },
    {
      title: "Complaints",
      paragraphs: [
        "If you discover errors or defects in the delivery, please notify us as soon as possible, and no later than within a reasonable time after the error was or should have been discovered. We will correct justified errors without undue delay.",
      ],
    },
    {
      title: "Limitation of liability",
      paragraphs: [
        "Our total liability for damages for a project is limited to the amount the customer has paid for the delivery in question, unless otherwise agreed in writing or the loss is caused by gross negligence or wilful misconduct.",
        "We are not liable for indirect losses, such as loss of profit or loss of data, beyond what follows from mandatory law.",
      ],
    },
    {
      title: "Copyright",
      paragraphs: [
        "Unless otherwise agreed, the customer obtains full rights of use to what has been developed specifically for the project once payment has been completed. TriodeLab retains the rights to its own tools, frameworks and generic code that is reused across projects.",
      ],
    },
    {
      title: "Governing law and disputes",
      paragraphs: [
        "These terms of sale are governed by Norwegian law. Disputes shall be sought resolved amicably, and shall otherwise be brought before the ordinary courts of TriodeLab’s legal venue in Norway.",
      ],
    },
  ],
};

export const legalDocumentsByLocale: Localized<{
  cookies: LegalDocument;
  privacy: LegalDocument;
  salesTerms: LegalDocument;
  terms: LegalDocument;
}> = {
  nb: {
    privacy: privacyDocument,
    cookies: cookiesDocument,
    terms: termsDocument,
    salesTerms: salesTermsDocument,
  },
  en: {
    privacy: privacyDocumentEn,
    cookies: cookiesDocumentEn,
    terms: termsDocumentEn,
    salesTerms: salesTermsDocumentEn,
  },
};
