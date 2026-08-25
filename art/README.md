# art/

Base artwork for this product, at source resolution. **Nothing here is
shipped as-is** — 1254x1254 at ~1.5 MB has no business in a page header or an
app bundle. Derivatives (header mark, favicon, app icon) get generated from
this and committed or built alongside; see task #115.

`hls-livecam-server.png` is this project's. The fork keeps its own in
`hls-lightcv-server`, deliberately: a node signs its licence notice and wears
its mark under its own name, and one shared art directory across two products
is how a fork ends up branded as the thing it forked from.

Source images are RGB with no alpha. The viewer header sits on a dark
housing, so a derivative needs a transparent or matched background rather
than the white one that comes out of a naive resize.

---

## macOS note (ariana)

Copied verbatim from `origin/main` for future derivative work. **Nothing here
is wired into the viewer or the bundle yet** — queued, per Ron.

Current macOS derivatives come from a *different* source and still do:

- `web/brand.png` / `web/brand@2x.png` — 40px/80px header mark
- `gui/assets/AppIcon.icns` — the .app bundle icon

Both were downscaled from `gui/assets/icon_1024.png`. When the switch happens
they should be regenerated from `hls-livecam-server.png` instead, and the
`.icns` rebuilt from its iconset rather than edited.

Heed the alpha warning above: this source is RGB with no alpha, and the
header sits on dark housing, so a naive `sips -Z` leaves a white square.

The LightCV artwork for **amira** is deliberately NOT here — it lives in the
`hls-lightcv-server` fork, so a fork does not end up wearing the mark of the
thing it forked from. Fetch it from that repo when amira is set up.
