variable "keycloak_url" {
  description = "Base URL of the Keycloak server."
  type        = string
  default     = "http://localhost:8081"
}

variable "keycloak_admin_username" {
  description = "Keycloak bootstrap admin username (master realm)."
  type        = string
  default     = "admin"
}

variable "keycloak_admin_password" {
  description = "Keycloak bootstrap admin password (master realm)."
  type        = string
  sensitive   = true
  default     = "admin"
}

variable "app_base_url" {
  description = "Base URL the SaaS app is reachable at, used for OIDC redirect URIs."
  type        = string
  default     = "http://localhost:8080"
}

variable "extra_app_base_urls" {
  description = "Additional base URLs the app may be reached at (e.g. LAN/Tailscale IP for testing from other devices), added alongside app_base_url."
  type        = list(string)
  default     = []
}

variable "google_oauth_client_id" {
  description = "Google OAuth 2.0 client ID (opendiy GCP project) used for the realm's Google identity provider broker."
  type        = string
}

variable "google_oauth_client_secret" {
  description = "Google OAuth 2.0 client secret used for the realm's Google identity provider broker."
  type        = string
  sensitive   = true
}
