use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cynic::{MutationBuilder, QueryBuilder};
#[cfg(test)]
use mockall::{automock, predicate::*};
use warp_graphql::mutations::stripe_billing_portal::{
    StripeBillingPortal, StripeBillingPortalInput, StripeBillingPortalResult,
    StripeBillingPortalVariables,
};

use super::ServerApi;
use crate::server::graphql::{get_request_context, get_user_facing_error_message};
use crate::server::ids::ServerId;


#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait WorkspaceClient: 'static + Send + Sync {
    async fn generate_stripe_billing_portal_link(&self, team_uid: ServerId) -> Result<String>;




}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl WorkspaceClient for ServerApi {
    async fn generate_stripe_billing_portal_link(&self, team_uid: ServerId) -> Result<String> {
        let variables = StripeBillingPortalVariables {
            input: StripeBillingPortalInput {
                team_uid: team_uid.into(),
            },
            request_context: get_request_context(),
        };
        let operation = StripeBillingPortal::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.stripe_billing_portal {
            StripeBillingPortalResult::StripeBillingPortalOutput(output) => Ok(output.url),
            StripeBillingPortalResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            StripeBillingPortalResult::Unknown => Err(anyhow!("Unknown error")),
        }
    }




}
