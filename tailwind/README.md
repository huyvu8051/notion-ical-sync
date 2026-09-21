# Tailwind build

Compiled ahead-of-time in Docker (see the `tailwind-builder` stage in the
root `Dockerfile`) instead of loading `cdn.tailwindcss.com` at runtime — the
Play CDN script was compiling the whole utility set in every visitor's
browser on every page load (~7s of render-blocking on a throttled mobile
connection).

## Two configs

| Config | Used by | Output |
|---|---|---|
| `auth-a.config.js` | every logged-in page (`/me`, sync log, the Notion onboarding flow, the calendar webview) | `/assets/style-auth-a.css` |
| `auth-b.config.js` | the public landing page (`/`) | `/assets/style-auth-b.css` |

The logged-in pages used to build against three separate configs
(`webview.config.js`, `oauth.config.js`, plus this one) with the same
Material-3-style token names mapped to different hex values per page —
organic drift rather than an intentional per-page palette. They were merged
into `auth-a.config.js` so every logged-in page shares one design and one
stylesheet.

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
