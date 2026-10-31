resource "keycloak_realm" "notion_caldav_saas" {
  realm   = "notion-caldav-saas"
  enabled = true

  display_name = "Notion CalDAV SaaS"

  # Users sign themselves up through Keycloak — no admin-created accounts.
  registration_allowed = true

  # The address is the identity. Signing up no longer asks for a username on
  # top of an email, and logging in takes the address rather than something
  # invented months ago and never typed since.
  #
  # The second flag is the one that was actually hurting: it was off, so an
  # account could only be reached through its username. People who signed up
  # in August are carrying names like "hllowrld" and "subz3r0", and forgetting
  # one locked them out of a service they could still name the email for.
  # Existing usernames are left alone — they keep working, and the address now
  # works beside them.
  registration_email_as_username = true
  login_with_email_allowed       = true

  # Required for the two above: an address cannot identify an account if two
  # accounts can share it.
  duplicate_emails_allowed = false
  reset_password_allowed         = true
  # On since 2026-11-01, once the SMTP block below could actually deliver.
  #
  # This applies to every login, not only new ones, so the 40 accounts that
  # existed at the time were marked emailVerified first — 33 of them real
  # people who had been using the service for months. Switching it on without
  # that would have put a wall between each of them and their own calendars
  # until they went looking for an email, and some would simply have left.
  # Those addresses are therefore trusted rather than proven; the point of
  # this flag is the people who sign up from here on.
  verify_email = true

  password_policy = "length(8)"

  # Keycloak sends its own mail and knows nothing about the Resend API the
  # app uses — this is a second path to the same provider, over SMTP. Without
  # it, anything Keycloak wants to email (verification, password reset) fails
  # at the login screen with no way for the person to recover.
  #
  # Same sender as the app's own mail, so a password reset and a receipt
  # arrive from one address rather than two.
  smtp_server {
    host              = "smtp.resend.com"
    port              = "587"
    starttls          = true
    from              = var.smtp_from
    from_display_name = "NotionCal"
    reply_to          = var.smtp_reply_to

    auth {
      # Resend's SMTP takes the literal username "resend" and an API key as
      # the password.
      username = "resend"
      password = var.smtp_password
    }
  }
}

resource "keycloak_openid_client" "app" {
  realm_id  = keycloak_realm.notion_caldav_saas.id
  client_id = "notion-caldav-saas-app"
  name      = "Notion CalDAV SaaS app"
  enabled   = true

  # Confidential: the axum backend holds the client secret and does the code exchange server-side.
  access_type = "CONFIDENTIAL"

  standard_flow_enabled        = true
  direct_access_grants_enabled = false
  implicit_flow_enabled        = false
  service_accounts_enabled     = false

  valid_redirect_uris = concat(
    ["${var.app_base_url}/*"],
    [for url in var.extra_app_base_urls : "${url}/*"],
  )
  web_origins = concat(
    [var.app_base_url],
    var.extra_app_base_urls,
  )
}

resource "keycloak_oidc_google_identity_provider" "google" {
  realm         = keycloak_realm.notion_caldav_saas.id
  client_id     = var.google_oauth_client_id
  client_secret = var.google_oauth_client_secret

  # Google accounts are already email-verified, so trust the address Keycloak
  # gets back instead of forcing a second verification step.
  trust_email    = true
  sync_mode      = "IMPORT"
  default_scopes = "openid email profile"

  # Without this, Google silently re-authenticates whichever account still
  # has an active browser session instead of showing the account chooser —
  # logging out of this app (or even Keycloak) never touches Google's own
  # session, so users could never switch Google accounts from the login page.
  extra_config = {
    prompt = "select_account"
  }
}
