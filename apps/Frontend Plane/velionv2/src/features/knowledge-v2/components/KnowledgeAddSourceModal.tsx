"use client";

import { useRef, useState } from "react";
import { ArrowUpRight, FileUp, FolderPlus, Globe2, HardDriveUpload } from "lucide-react";

import {
  VerevonButton,
  VerevonInput,
  VerevonModal,
  VerevonModalClose,
  VerevonModalTitle,
} from "@/components/ui/verevon-ui";

type ConnectProvider = {
  detail: string;
  id: string;
  label: string;
  sources: readonly string[];
};

type SharePointSourceForm = {
  driveId: string;
  driveName: string;
  driveType: string;
  siteId: string;
  siteWebUrl: string;
  tenantId: string;
};

type WebsiteCrawlForm = {
  maxPages: string;
  url: string;
};

export function KnowledgeAddSourceModal({
  busy,
  onClose,
  onConnectProvider,
  onRegisterSharePoint,
  onStartWebsiteCrawl,
  onUploadFiles,
  providers,
}: {
  busy: boolean;
  onClose: () => void;
  onConnectProvider: (provider: ConnectProvider) => Promise<void> | void;
  onRegisterSharePoint: (input: SharePointSourceForm) => Promise<void>;
  onStartWebsiteCrawl: (input: { maxPages?: number; url: string }) => Promise<void>;
  onUploadFiles: (files: File[]) => Promise<void>;
  providers: readonly ConnectProvider[];
}) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [selectedFiles, setSelectedFiles] = useState<File[]>([]);
  const [sharePoint, setSharePoint] = useState<SharePointSourceForm>({
    driveId: "",
    driveName: "",
    driveType: "documentLibrary",
    siteId: "",
    siteWebUrl: "",
    tenantId: "",
  });
  const [websiteCrawl, setWebsiteCrawl] = useState<WebsiteCrawlForm>({
    url: "",
    maxPages: "12",
  });

  async function submitUpload() {
    if (selectedFiles.length === 0) return;
    await onUploadFiles(selectedFiles);
    setSelectedFiles([]);
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  async function submitWebsiteCrawl() {
    const target = websiteCrawl.url.trim();
    if (!target) return;
    const parsedMaxPages = Number.parseInt(websiteCrawl.maxPages, 10);
    await onStartWebsiteCrawl({
      url: target,
      maxPages: Number.isFinite(parsedMaxPages) ? parsedMaxPages : undefined,
    });
    setWebsiteCrawl({ url: "", maxPages: "12" });
  }

  return (
    <VerevonModal label="Add knowledge source" size="wide" className="p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <VerevonModalTitle>Add source</VerevonModalTitle>
          <p className="mt-2 max-w-2xl text-[13px] leading-5 text-[#666B64] dark:text-[#AEB4C0]">
            Upload files through imports-core, open new integration auth flows, or register a SharePoint drive for Finspo sync.
          </p>
        </div>
        <VerevonModalClose onClick={onClose} aria-label="Close add source">Esc</VerevonModalClose>
      </div>

      <div className="mt-5 grid gap-4 xl:grid-cols-[minmax(0,1.05fr)_minmax(320px,0.95fr)]">
        <section className="rounded-[18px] border border-[#E3E1DA] bg-[#FAFAF8] p-4 dark:border-[#30333A] dark:bg-[#101114]">
          <div className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-[10px] bg-[#F0EFE9] text-[#171A16] dark:bg-[#17191E] dark:text-white">
              <FileUp className="size-5" />
            </span>
            <div>
              <h3 className="text-[16px] font-semibold text-[#171A16] dark:text-white">Upload files</h3>
              <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">
                Creates an imports-core job and pushes the files into Data Plane v2.
              </p>
            </div>
          </div>

          <input
            ref={fileInputRef}
            type="file"
            multiple
            className="mt-4 block w-full text-[13px] text-[#3B3F38] file:mr-3 file:rounded-[8px] file:border-0 file:bg-[#171A16] file:px-3 file:py-2 file:text-white dark:text-[#D9DEE7] dark:file:bg-white dark:file:text-[#111111]"
            onChange={(event) => setSelectedFiles(Array.from(event.target.files ?? []))}
          />

          <div className="mt-3 flex flex-wrap gap-2">
            {selectedFiles.length > 0 ? selectedFiles.map((file) => (
              <span
                key={`${file.name}-${file.size}`}
                className="rounded-full bg-black/[0.05] px-2.5 py-1 text-[11px] text-[#5E635B] dark:bg-white/10 dark:text-[#DDE3ED]"
              >
                {file.name}
              </span>
            )) : (
              <span className="text-[12px] text-[#858980] dark:text-[#8F96A3]">No files selected yet.</span>
            )}
          </div>

          <div className="mt-4">
            <VerevonButton
              radius="sm"
              variant="primary"
              className="px-3"
              disabled={busy || selectedFiles.length === 0}
              onClick={() => void submitUpload()}
            >
              <HardDriveUpload className="size-4" />
              Import selected files
            </VerevonButton>
          </div>
        </section>

        <section className="rounded-[18px] border border-[#E3E1DA] bg-[#FAFAF8] p-4 dark:border-[#30333A] dark:bg-[#101114]">
          <div className="flex items-center gap-3">
            <span className="grid size-10 place-items-center rounded-[10px] bg-[#F0EFE9] text-[#171A16] dark:bg-[#17191E] dark:text-white">
              <ArrowUpRight className="size-5" />
            </span>
            <div>
              <h3 className="text-[16px] font-semibold text-[#171A16] dark:text-white">Connect a workspace</h3>
              <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">
                Starts a live integration-core OAuth session in a new window.
              </p>
            </div>
          </div>

          <div className="mt-4 grid gap-2 sm:grid-cols-2">
            {providers.map((provider) => (
              <button
                key={provider.id}
                type="button"
                disabled={busy}
                onClick={() => void onConnectProvider(provider)}
                className="rounded-[12px] border border-[#E2DFD5] bg-white px-3 py-3 text-left transition-colors hover:border-[#C9C3B3] hover:bg-[#F7F4EB] disabled:cursor-not-allowed disabled:opacity-60 dark:border-[#30333A] dark:bg-[#16181D] dark:hover:border-[#454A55] dark:hover:bg-[#1B1D22]"
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="text-[13px] font-semibold text-[#171A16] dark:text-white">{provider.label}</span>
                  <ArrowUpRight className="size-4 text-[#6B6F67] dark:text-[#AEB4C0]" />
                </div>
                <p className="mt-1 text-[12px] leading-5 text-[#74786F] dark:text-[#9EA3AD]">{provider.detail}</p>
              </button>
            ))}
          </div>
        </section>
      </div>

      <section className="mt-4 rounded-[18px] border border-[#E3E1DA] bg-[#FAFAF8] p-4 dark:border-[#30333A] dark:bg-[#101114]">
        <div className="flex items-center gap-3">
          <span className="grid size-10 place-items-center rounded-[10px] bg-[#F0EFE9] text-[#171A16] dark:bg-[#17191E] dark:text-white">
            <Globe2 className="size-5" />
          </span>
          <div>
            <h3 className="text-[16px] font-semibold text-[#171A16] dark:text-white">Crawl a website</h3>
            <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">
              Starts a Quarry crawl so website pages can flow into Knowledge through the ingestion stack.
            </p>
          </div>
        </div>

        <div className="mt-4 grid gap-3 md:grid-cols-[minmax(0,1fr)_140px]">
          <Field
            label="Website URL"
            value={websiteCrawl.url}
            onChange={(value) => setWebsiteCrawl((current) => ({ ...current, url: value }))}
            placeholder="https://docs.verevon.ai"
          />
          <Field
            label="Max pages"
            value={websiteCrawl.maxPages}
            onChange={(value) => setWebsiteCrawl((current) => ({ ...current, maxPages: value }))}
            placeholder="12"
          />
        </div>

        <div className="mt-4">
          <VerevonButton
            radius="sm"
            variant="primary"
            className="px-3"
            disabled={busy || !websiteCrawl.url.trim()}
            onClick={() => void submitWebsiteCrawl()}
          >
            <Globe2 className="size-4" />
            Start crawl
          </VerevonButton>
        </div>
      </section>

      <section className="mt-4 rounded-[18px] border border-[#E3E1DA] bg-[#FAFAF8] p-4 dark:border-[#30333A] dark:bg-[#101114]">
        <div className="flex items-center gap-3">
          <span className="grid size-10 place-items-center rounded-[10px] bg-[#F0EFE9] text-[#171A16] dark:bg-[#17191E] dark:text-white">
            <FolderPlus className="size-5" />
          </span>
          <div>
            <h3 className="text-[16px] font-semibold text-[#171A16] dark:text-white">Register SharePoint drive</h3>
            <p className="mt-1 text-[12px] text-[#74786F] dark:text-[#9EA3AD]">
              Persists a Finspo source and immediately starts a SharePoint or OneDrive sync.
            </p>
          </div>
        </div>

        <div className="mt-4 grid gap-3 md:grid-cols-2">
          <Field
            label="Site ID"
            value={sharePoint.siteId}
            onChange={(value) => setSharePoint((current) => ({ ...current, siteId: value }))}
            placeholder="contoso.sharepoint.com,site-id,web-id"
          />
          <Field
            label="Drive ID"
            value={sharePoint.driveId}
            onChange={(value) => setSharePoint((current) => ({ ...current, driveId: value }))}
            placeholder="b!drive-id"
          />
          <Field
            label="Drive name"
            value={sharePoint.driveName}
            onChange={(value) => setSharePoint((current) => ({ ...current, driveName: value }))}
            placeholder="Support knowledge"
          />
          <Field
            label="Drive type"
            value={sharePoint.driveType}
            onChange={(value) => setSharePoint((current) => ({ ...current, driveType: value }))}
            placeholder="documentLibrary"
          />
          <Field
            label="Site URL"
            value={sharePoint.siteWebUrl}
            onChange={(value) => setSharePoint((current) => ({ ...current, siteWebUrl: value }))}
            placeholder="https://contoso.sharepoint.com/sites/Support"
          />
          <Field
            label="Tenant ID"
            value={sharePoint.tenantId}
            onChange={(value) => setSharePoint((current) => ({ ...current, tenantId: value }))}
            placeholder="Optional"
          />
        </div>

        <div className="mt-4">
          <VerevonButton
            radius="sm"
            variant="primary"
            className="px-3"
            disabled={busy || !sharePoint.siteId.trim() || !sharePoint.driveId.trim()}
            onClick={() => void onRegisterSharePoint(sharePoint)}
          >
            <FolderPlus className="size-4" />
            Register and sync
          </VerevonButton>
        </div>
      </section>
    </VerevonModal>
  );
}

function Field({
  label,
  onChange,
  placeholder,
  value,
}: {
  label: string;
  onChange: (value: string) => void;
  placeholder: string;
  value: string;
}) {
  return (
    <label className="space-y-1.5">
      <span className="text-[12px] font-medium text-[#555A52] dark:text-[#AEB4C0]">{label}</span>
      <VerevonInput
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}
