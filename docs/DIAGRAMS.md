# System diagrams

All diagrams here are [Mermaid](https://mermaid.js.org/) code blocks, not
images — GitHub renders them natively, no extra tooling needed to view them.

- [Use case diagram](#use-case-diagram) — who uses NotionCal, and for what
- [Sequence diagram](#sequence-diagram--two-way-sync) — how a calendar app and Notion stay in sync
- [Activity diagram](#activity-diagram--connecting-a-notion-database) — the connect-a-database flow, decision by decision
- [Component diagram](#component-diagram) — how the axum service is put together internally
- [Deployment diagram](#deployment-diagram) — where it actually runs

## Use case diagram

Who uses NotionCal, and what each actor can do.

```mermaid
flowchart LR
    User((User))
    CalendarApp["Calendar App<br/>«client»"]
    Google["Google<br/>«identity»"]
    Notion["Notion<br/>«system»"]
    Stripe["Stripe<br/>«billing»"]
    Admin((Admin))

    subgraph NotionCal["NotionCal «system»"]
        subgraph Account["ACCOUNT"]
            UC1(["Sign up / log in"])
            UC2(["Log in with Google"])
        end
        subgraph Calendars["CALENDARS"]
            UC3(["Connect Notion database"])
            UC4(["View calendars"])
            UC5(["Manage CalDAV credentials"])
            UC6(["Delete calendar"])
            UC7(["View sync log"])
        end
        subgraph Sync["SYNC"]
            UC8(["Sync events (Notion ⇄ CalDAV)"])
        end
        subgraph Events["EVENTS"]
            UC9(["View calendar"])
            UC10(["Create event"])
            UC11(["Edit / reschedule event"])
            UC12(["Delete event"])
        end
        subgraph Billing["BILLING"]
            UC13(["Start free trial"])
            UC14(["Subscribe"])
            UC15(["Reset billing<br/>(admin only)"])
        end
    end

    User --> Account
    User --> Calendars
    User --> Events
    User --> UC14

    CalendarApp --> UC8
    Google --> UC2
    Notion --> UC3
    Notion --> UC8
    Stripe --> UC14
    Admin --> UC15

    UC2 -. "«extend»" .-> UC1
    UC1 -. "«include»" .-> UC13
```

## Actors

| Actor | Role | Touches |
| --- | --- | --- |
| **User** | Primary actor — the person syncing a Notion database to their calendar app | Account, Calendars, Events, Subscribe |
| **Calendar App** | Apple Calendar / Google Calendar / any CalDAV client | Pulls the synced feed |
| **Google** | OAuth identity provider (via the Keycloak broker) | Log in with Google |
| **Notion** | External API + webhooks | Connect Notion database, Sync events |
| **Stripe** | Payments | Subscribe |
| **Admin** | Operator, authenticated via the `X-Admin-Secret` header | Reset billing (`POST /admin/reset-billing`) — dev/test tool, 404s unless `ADMIN_SECRET` is configured |

### Relationships

- **Solid line** — an actor directly uses a use case (or a whole group of them).
- **`«extend»`** — Log in with Google is an optional variant of Sign up / log in.
- **`«include»`** — Sign up / log in always starts a free trial.

## Sequence diagram — two-way sync

The core of the product: a CalDAV client can pull the feed at any time, a
write from either side propagates to the other, and a Notion edit shows up
without the client having to do anything but poll.

```mermaid
sequenceDiagram
    actor Client as Calendar App
    participant Server as NotionCal
    participant DB as Postgres
    participant Notion as Notion API

    Note over Client,Server: Pull — client polls periodically
    Client->>Server: GET /cal/{id} (CalDAV report)
    Server->>DB: load cached events for calendar
    DB-->>Server: event rows
    Server->>Server: build_ics()
    Server-->>Client: 200 OK + iCalendar feed

    Note over Client,Notion: Push — client creates/edits an event
    Client->>Server: PUT /cal/{id}/{event}.ics
    Server->>Server: parse_ics_to_page_info()
    Server->>Notion: create or update page
    Notion-->>Server: page id + properties
    Server->>DB: upsert event mapping
    Server-->>Client: 201 Created / 204 No Content

    Note over Notion,Server: Push — user edits the page in Notion instead
    Notion->>Server: webhook: page updated
    Server->>Notion: fetch updated page properties
    Server->>Server: build_ics()
    Server->>DB: update cached event
    Client->>Server: GET /cal/{id} (next poll)
    Server-->>Client: 200 OK + updated feed
```

## Activity diagram — connecting a Notion database

What happens, decision by decision, between clicking **Connect another
database** and seeing it show up in the calendar list.

```mermaid
flowchart TD
    start((Start)) --> clickBtn["User clicks Connect another database"]
    clickBtn --> oauth[Redirect to Notion OAuth consent]
    oauth --> approved{User approves<br/>access?}
    approved -- No --> cancelled[Show connection-cancelled message]
    cancelled --> stop1((End))

    approved -- Yes --> exchange[Exchange code for access token]
    exchange --> list[List accessible databases]
    list --> pick[User picks a database]
    pick --> quota{Within calendar<br/>quota for plan?}

    quota -- No --> upsell[Show upgrade prompt]
    upsell --> stop2((End))

    quota -- Yes --> schema[Read database schema]
    schema --> compatible{Has a date<br/>property?}
    compatible -- No --> pickAnother[Ask user to pick<br/>a different property/database]
    pickAnother --> pick

    compatible -- Yes --> create[Create calendar record +<br/>generate CalDAV credentials]
    create --> initial[Run initial sync]
    initial --> show[Show calendar in the calendars list]
    show --> stop3((End))
```

## Component diagram

Every page except the small interactive widgets (the confirm buttons — see
`crates/islands`) is still plain server-rendered HTML from these axum
handler modules; nothing runs through a client-side framework for routing.

```mermaid
flowchart TB
    Browser["Browser<br/>(server-rendered HTML +<br/>Leptos islands over WASM)"]

    subgraph Service["notion-ical-sync (axum)"]
        Router["caldav.rs<br/>router + CalDAV protocol"]
        Auth["auth.rs<br/>account / calendars page"]
        OAuth["oauth.rs<br/>OIDC + Notion OAuth callbacks"]
        Webview["webview.rs<br/>FullCalendar UI + event API"]
        Billing["billing.rs<br/>checkout, webhooks, admin reset"]
        Legal["legal.rs<br/>privacy / terms"]
        Islands["islands crate<br/>compiled to WASM, served at /pkg"]
    end

    Postgres[("Postgres<br/>users, calendars, events, sessions")]
    NotionAPI["Notion API"]
    Stripe["Stripe API"]
    Keycloak["Keycloak<br/>(OIDC broker incl. Google)"]
    SMTP["SMTP<br/>(trial reminder emails)"]

    Browser <--> Router
    Router --> Auth
    Router --> OAuth
    Router --> Webview
    Router --> Billing
    Router --> Legal
    Router -. static files .-> Islands
    Islands -. wasm bundle .-> Browser

    Auth --> Postgres
    OAuth --> Postgres
    OAuth --> Keycloak
    OAuth --> NotionAPI
    Webview --> Postgres
    Webview --> NotionAPI
    Billing --> Postgres
    Billing --> Stripe

    Service -. background jobs .-> NotionAPI
    Service -. background jobs .-> SMTP
```

## Deployment diagram

Push to `main` builds a Docker image (server binary + a separate
wasm-bindgen build of the islands crate) and ships it straight to
production — see `.github/workflows/ci.yml` and the `Dockerfile`.

```mermaid
flowchart TB
    subgraph GitHub["GitHub"]
        Repo["notion-ical-sync repo"]
        Actions["GitHub Actions<br/>build-and-push"]
    end
    Registry[("ghcr.io<br/>container registry")]

    subgraph Cluster["k3s cluster"]
        ArgoCD["ArgoCD<br/>(App-of-Apps, syncs from git)"]
        subgraph Pods["Deployment (2 replicas, rolling update)"]
            Pod1["Pod"]
            Pod2["Pod"]
        end
        PG[("Postgres pod<br/>notion-caldav-saas-db")]
        Secrets["Sealed Secrets<br/>ADMIN_SECRET, CALDAV_PASSWORD_ENC_KEY, ..."]
    end

    Cloudflare["Cloudflare<br/>tunnel + edge cache"]
    BrowserUser(["User's browser"])
    ClientApp(["Calendar app<br/>(CalDAV client)"])

    Keycloak["Keycloak<br/>auth.opendiy.vn"]
    Stripe["Stripe"]
    Notion["Notion API"]

    Repo -- push to main --> Actions
    Actions -- docker build --> Registry
    ArgoCD -- watches --> Repo
    ArgoCD -- deploys --> Pods
    Registry -. image pull .-> Pods

    BrowserUser --> Cloudflare
    ClientApp --> Cloudflare
    Cloudflare -- tunnel --> Pods
    Pods --> PG
    Pods --> Secrets
    Pods --> Keycloak
    Pods --> Stripe
    Pods --> Notion
```
