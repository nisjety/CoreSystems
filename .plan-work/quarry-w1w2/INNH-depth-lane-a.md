# INNH depth findings, lane A (scrape / extract)

One line per finding. Things worth doing later; none is needed to close the item's gap.

INNH-1 | The privacy skip happens after the page fetch (the scrape is still served); a request that is both AI-formatted and credential_or_secret could skip the model lane checks (501s) as ZDR does, but today a deployment without a model still answers 501 before the fetch for such a request. Decide whether the privacy block should behave like ZDR there.
INNH-1 | The ZDR matrix rows for scrape and extract do not name the PRIVACY_POLICY skip (it adds no persistence or new content surface, so the golden was not regenerated); add it to the basis text next time the golden is regenerated.
INNH-1 | /v1/extract reports a privacy-block refusal as a per-source error string starting PRIVACY_POLICY:, like ZDR_UNAVAILABLE:, not as a structured code; a typed code on ExtractItem would be cleaner for the UI.
