/**
 * Saves bytes to the user's downloads as a file, entirely client-side: a
 * `Blob` + object URL + a synthetic anchor click, revoked right after. No
 * network request, no server round trip — the bytes this module hands the
 * browser are the same bytes `docx.ts`/`xlsx.ts`/`pptx.ts` built in memory.
 */
export function downloadBytes(bytes: Uint8Array, fileName: string, mime: string): void {
  const blob = new Blob([new Uint8Array(bytes)], { type: mime })
  const url = URL.createObjectURL(blob)
  try {
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = fileName
    // Firefox only fires a click that isn't attached to the document in some
    // versions; attaching briefly costs nothing and works everywhere.
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
  } finally {
    // Deferred: revoking the URL synchronously has been observed to cancel
    // the download in flight on some browsers, since the click's navigation
    // hasn't necessarily finished reading the blob yet.
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
}
