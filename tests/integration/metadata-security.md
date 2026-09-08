# Integration note: metadata security

**Status:** specification harness. Metadata fetcher is **not implemented**. These cases must pass before any URI from chain metadata is fetched or shown.

## Threat

Token `uri` fields and linked content are hostile: SSRF (loopback, link-local, cloud metadata), open redirects, huge bodies, SVG script, HTML/JS in the webview.

## Required controls (spec 08)

- Allow `https` and an explicit gateway allow-list only.
- Resolve DNS; reject loopback, private, link-local, multicast, cloud-metadata ranges **before every hop**.
- Max three redirects; revalidate each destination.
- Strict size, content-type and timeout.
- Store hash + sanitized fields. Never auto-render remote HTML.
- Images: decode and re-encode through a cache; strip or rasterize SVG.
- No automatic crawl of external sites.

## Cases to run when the fetcher exists

| URI / payload | Expected |
|---|---|
| `http://127.0.0.1/` | Blocked |
| `http://169.254.169.254/` | Blocked |
| DNS to a private A record | Blocked after resolve |
| Redirect chain to loopback | Blocked on hop |
| `javascript:` / `data:text/html` | Rejected |
| SVG with `<script>` | Not executed; raster or reject |
| 100 MB body | Truncate/timeout; hash of what was read |
| Valid small PNG | Proxied re-encode; original not inlined |

## UI

Evidence drawer shows the blocked URI reason and a neutral placeholder. Quality lists `metadata` as stale/missing. No invented name/symbol from a failed fetch if the chain event did not carry them.

## Paper / models

A hostile URI must not change predictions, policies or ledger state. Investigator (when present) may cite the **block reason**, not the fetched HTML.
