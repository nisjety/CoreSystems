/// <reference types="vite/client" />

// Eksplisitt typet i stedet for Vite sin `any`-indekssignatur, slik at
// manglende oppsett fanges av typesjekken og ikke blir stille `any`.
interface ImportMetaEnv {
  readonly VITE_REMOTE_RENDEZVOUS_URL?: string;
  readonly VITE_REMOTE_RELAY_URL?: string;
  readonly VITE_REMOTE_SERVER_PUBLIC_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
