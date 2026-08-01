output "realm" {
  value = keycloak_realm.notion_caldav_saas.realm
}

output "client_id" {
  value = keycloak_openid_client.app.client_id
}

output "client_secret" {
  value     = keycloak_openid_client.app.client_secret
  sensitive = true
}

output "issuer_url" {
  value = "${var.keycloak_url}/realms/${keycloak_realm.notion_caldav_saas.realm}"
}
