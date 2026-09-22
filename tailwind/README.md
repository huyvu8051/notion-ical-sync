# Tailwind build

Compiled ahead-of-time in Docker (see the `tailwind-builder` stage in the
root `Dockerfile`) instead of loading `cdn.tailwindcss.com` at runtime — the
Play CDN script was compiling the whole utility set in every visitor's
browser on every page load (~7s of render-blocking on a throttled mobile
connection).

## One config

`auth-a.config.js` builds `/assets/style-auth-a.css`, used by every page —
the public landing page included. It used to be three separate logged-in
configs (`webview.config.js`, `oauth.config.js`, plus this one), then a
fourth (`auth-b.config.js`) just for the landing page — same Material-3-style
token names mapped to slightly different values per page, organic drift
rather than an intentional per-page palette each time. All four are now
merged into this one config.

Serving every page from the same stylesheet file also matters for
client-side navigation: when a route change swaps the `<link>` tag for a
*different* CSS file, the browser can render a flash of unstyled content
while that file downloads. Splitting pages across stylesheets should be
a deliberate call, not a side effect of copy-pasting a config.

## Editing Tailwind classes

`content` in each config globs `../src/**/*.rs` and `../crates/**/*.rs`, so
any class name written in a Rust template is picked up automatically —
no separate step needed beyond rebuilding the Docker image (or running the
command below locally).

To rebuild a stylesheet locally without Docker:

```sh
cd tailwind
npx tailwindcss@3.4.17 -c auth-a.config.js -i input.css -o ../assets/style-auth-a.css --minify
```
