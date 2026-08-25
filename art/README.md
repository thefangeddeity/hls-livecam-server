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
