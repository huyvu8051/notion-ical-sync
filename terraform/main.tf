resource "keycloak_realm" "notion_caldav_saas" {
  realm   = "notion-caldav-saas"
  enabled = true

  display_name = "Notion CalDAV SaaS"

  # Users sign themselves up through Keycloak — no admin-created accounts.
  registration_allowed           = true
  registration_email_as_username = false
  reset_password_allowed         = true
  verify_email                   = false # tighten once real email delivery is set up

  password_policy = "length(8)"
}

resource "keycloak_openid_client" "app" {
  realm_id  = keycloak_realm.notion_caldav_saas.id
  client_id = "notion-caldav-saas-app"
  name      = "Notion CalDAV SaaS app"
  enabled   = true

  # Confidential: the axum backend holds the client secret and does the
  # authorization-code exchange server-side — nothing OIDC-related runs in
  # the browser/wasm (this app has no wasm build at all, see webview.rs).
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
}
