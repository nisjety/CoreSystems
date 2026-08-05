export function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('Unable to read blob.'))
    reader.readAsDataURL(blob)
  })
}

export function objectUrlToDataUrl(url: string): Promise<string> {
  if (!url.startsWith('blob:')) {
    return Promise.reject(new TypeError('Expected a browser object URL.'))
  }

  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest()
    request.open('GET', url)
    request.responseType = 'blob'
    request.onload = () => {
      const status = request.status
      if (status !== 0 && (status < 200 || status >= 300)) {
        reject(new Error('Unable to read object URL.'))
        return
      }

      const blob = request.response
      if (!(blob instanceof Blob)) {
        reject(new Error('Object URL did not resolve to a blob.'))
        return
      }

      void blobToDataUrl(blob).then(resolve, reject)
    }
    request.onerror = () => reject(new Error('Unable to read object URL.'))
    request.onabort = () => reject(new Error('Object URL read was aborted.'))
    request.send()
  })
}

export function parseDataUrl(dataUrl: string): { base64: string; mime: string } | null {
  const commaIndex = dataUrl.indexOf(',')
  if (!dataUrl.startsWith('data:') || commaIndex < 0) return null

  const mime = dataUrl.slice(5, commaIndex).split(';')[0] || 'application/octet-stream'
  const base64 = dataUrl.slice(commaIndex + 1)
  return base64 ? { base64, mime } : null
}
