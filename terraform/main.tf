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

# The registration form, declared rather than clicked.
#
# Keycloak 26 builds that form from this profile: a field appears only when
# the person filling it in is allowed to edit the attribute. So first and last
# name are not removed here — they are simply no longer editable by the user,
# which takes them off the form while leaving them writable by an admin.
#
# That distinction is the whole point. Deleting the attributes would be the
# obvious move and the wrong one: this realm has no unmanagedAttributePolicy,
# so an attribute nobody declares is an attribute Keycloak rejects — and 43 of
# the accounts here sign in through Google, which sends given_name and
# family_name on every login.
#
# Username is absent from the form for a different reason: the realm sets
# registration_email_as_username, so Keycloak fills it from the address.
resource "keycloak_realm_user_profile" "notion_caldav_saas" {
  realm_id = keycloak_realm.notion_caldav_saas.id

  attribute {
    name         = "username"
    display_name = "$${username}"

    permissions {
      view = ["admin", "user"]
      edit = ["admin", "user"]
    }

    validator {
      name = "length"
      config = {
        min = 3
        max = 255
      }
    }
    validator { name = "username-prohibited-characters" }
    validator { name = "up-username-not-idn-homograph" }
  }

  attribute {
    name           = "email"
    display_name   = "$${email}"
    required_for_roles = ["user"]

    permissions {
      view = ["admin", "user"]
      edit = ["admin", "user"]
    }

    validator { name = "email" }
    validator {
      name   = "length"
      config = { max = 255 }
    }
  }

  # No required_for_roles and no "user" in edit: off the form, still set by
  # the Google login that supplies them.
  attribute {
    name         = "firstName"
    display_name = "$${firstName}"

    permissions {
      view = ["admin", "user"]
      edit = ["admin"]
    }

    validator {
      name   = "length"
      config = { max = 255 }
    }
    validator { name = "person-name-prohibited-characters" }
  }

  attribute {
    name         = "lastName"
    display_name = "$${lastName}"

    permissions {
      view = ["admin", "user"]
      edit = ["admin"]
    }

    validator {
      name   = "length"
      config = { max = 255 }
    }
    validator { name = "person-name-prohibited-characters" }
  }

  group {
    name                = "user-metadata"
    display_header      = "User metadata"
    display_description = "Attributes, which refer to user metadata"
  }
}
