//! Workspace-level billing/plan accessors: reads of `workspace.billing_metadata` (tier and
//! policy entitlements) that hold regardless of which team a window has selected. See
//! [`crate::workspaces::user_workspaces::team_workspace_settings`] for the workspace-vs-team
//! two-layer model and the team-scoped policies that layer on top.

use super::UserWorkspaces;
#[cfg(test)]
use crate::workspaces::workspace::PurchaseAddOnCreditsPolicy;

impl UserWorkspaces {
    #[cfg(test)]
    pub fn purchase_policy(&self) -> Option<PurchaseAddOnCreditsPolicy> {
        self.current_workspace()
            .and_then(|workspace| workspace.billing_metadata.tier.purchase_add_on_credits_policy)
            .or(self.user_purchase_policy)
    }
}
