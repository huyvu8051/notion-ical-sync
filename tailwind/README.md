# Tailwind build

Compiled ahead-of-time in Docker (see the `tailwind-builder` stage in the
root `Dockerfile`) instead of loading `cdn.tailwindcss.com` at runtime — the
Play CDN script was compiling the whole utility set in every visitor's
browser on every page load (~7s of render-blocking on a throttled mobile
connection).

## Why 4 configs instead of 1

`src/webview.rs`, `src/auth.rs` (two page states), and `src/oauth.rs` each
had their own inline `tailwind.config` with the same Material-3-style token
names (`secondary`, `outline-variant`, ...) mapped to **different hex
values** per page — e.g. `secondary` is `#3B82F6` in the calendar webview
but `#0058be` in auth/oauth. That looks like organic drift rather than an
intentional per-page palette, but unifying it is a design decision, not a
performance fix, so each page keeps building against its own config and its
own compiled stylesheet:

| Config | Used by | Output |
|---|---|---|
| `webview.config.js` | `src/webview.rs` | `/assets/style-webview.css` |
| `auth-a.config.js` | `src/auth.rs` (line ~197 block) | `/assets/style-auth-a.css` |
| `auth-b.config.js` | `src/auth.rs` (line ~666/906 blocks) | `/assets/style-auth-b.css` |
| `oauth.config.js` | `src/oauth.rs` | `/assets/style-oauth.css` |

If you deliberately unify the palette later, collapsing these back into one
config + one stylesheet is straightforward.

## Editing Tailwind classes

`content` in each config globs `../src/**/*.rs` and `../crates/**/*.rs`, so
any class name written in a Rust template is picked up automatically —
no separate step needed beyond rebuilding the Docker image (or running the
command below locally).

To rebuild a stylesheet locally without Docker:

```sh
cd tailwind
npx tailwindcss@3.4.17 -c webview.config.js -i input.css -o ../assets/style-webview.css --minify
```
