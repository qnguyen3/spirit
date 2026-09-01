//! Workspace-level billing/plan accessors: reads of `workspace.billing_metadata` (tier and
//! policy entitlements) that hold regardless of which team a window has selected. See
//! [`crate::workspaces::user_workspaces::team_workspace_settings`] for the workspace-vs-team
//! two-layer model and the team-scoped policies that layer on top.


use super::UserWorkspaces;

impl UserWorkspaces {
}
