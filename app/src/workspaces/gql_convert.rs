use anyhow::{Result, anyhow, bail};
use regex::Regex;
use warp_errors::report_error;
use warp_graphql::billing::{
    AiAutonomyPolicy as GqlAiAutonomyPolicy, AmbientAgentsPolicy as GqlAmbientAgentsPolicy,
    BillingCycleUsageHistory as GqlBillingCycleUsageHistory, BillingMetadata as GqlBillingMetadata,
    ByoApiKeyPolicy as GqlByoApiKeyPolicy, ByoEndpointPolicy as GqlByoEndpointPolicy,
    CodebaseContextPolicy as GqlCodebaseContextPolicy, CustomerType as GqlCustomerType,
    DelinquencyStatus as GqlDelinquencyStatus,
    EnterpriseCreditsAutoReloadPolicy as GqlEnterpriseCreditsAutoReloadPolicy,
    EnterprisePayAsYouGoPolicy as GqlEnterprisePayAsYouGoPolicy, InstanceShape as GqlInstanceShape,
    ManagedByokByoePolicy as GqlManagedByokByoePolicy, MultiAdminPolicy as GqlMultiAdminPolicy,
    NativeWorkspacesPolicy as GqlNativeWorkspacesPolicy,
    PurchaseAddOnCreditsPolicy as GqlPurchaseAddOnCreditsPolicy, ServiceAgreementType,
    SessionSharingPolicy as GqlSessionSharingPolicy,
    SharedNotebooksPolicy as GqlSharedNotebooksPolicy,
    SharedWorkflowsPolicy as GqlSharedWorkflowsPolicy, StripeSubscriptionPlan,
    TeamSizePolicy as GqlTeamSizePolicy,
    TelemetryDataCollectionPolicy as GqlTelemetryDataCollectionPolicy, Tier as GqlTier,
    UgcDataCollectionPolicy as GqlUgcDataCollectionPolicy,
    UsageBasedPricingPolicy as GqlUsageBasedPricingPolicy,
    UsageVisibilityGranularity as GqlUsageVisibilityGranularity,
    UsageVisibilityPolicy as GqlUsageVisibilityPolicy, WarpAiPolicy as GqlWarpAiPolicy,
};
use warp_graphql::queries::get_workspaces_metadata_for_user::User as GqlUser;
use warp_graphql::subscriptions::get_warp_drive_updates::WarpDriveUpdate;
use warp_graphql::user::DiscoverableTeamData as GqlDiscoverableTeamData;
use warp_graphql::workspace::{
    AddonCreditsSettings as GqlAddonCreditsSettings,
    AdminEnablementSetting as GqlAdminEnablementSetting,
    AiPermissionsSettings as GqlAiPermissionsSettings,
    ByoEndpointMetadata as GqlByoEndpointMetadata,
    ByoEndpointModelMetadata as GqlByoEndpointModelMetadata,
    ByoFirstPartyKey as GqlByoFirstPartyKey, EmailInvite as GqlEmailInvite,
    InviteLinkDomainRestriction as GqlInviteLinkDomainRestriction,
    MembershipRole as GqlMembershipRole, Team as GqlTeam,
    TeamByoSettings as GqlTeamByoSettings, TeamMember as GqlTeamMember,
    TeamSettings as GqlTeamSettings, TeamVisibility as GqlTeamVisibility,
    UgcCollectionEnablementSetting as GqlUgcCollectionEnablementSetting, Workspace as GqlWorkspace,
    WorkspaceMember as GqlWorkspaceMember, WorkspaceMemberUsageInfo as GqlWorkspaceMemberUsageInfo,
    WorkspaceSettings as GqlWorkspaceSettings,
};

use super::team::{DiscoverableTeam, MembershipRole, Team, TeamMember, TeamVisibility};
use super::user_workspaces::WorkspacesMetadataResponse;
use super::workspace::{
    AIAutonomyPolicy, AddonCreditsSettings, AdminEnablementSetting, AiPermissionsSettings,
    AmbientAgentsPolicy, BillingCycleUsageData, BillingCycleUsageEntry, BillingCycleUsageSummary,
    BillingMetadata, ByoEndpointMetadata, ByoEndpointModelMetadata, ByoFirstPartyKey,
    CloudConversationStorageSettings, CodebaseContextSettings, CustomerType, DelinquencyStatus,
    EmailInvite, EnforceableSetting, EnterpriseSecretRegex, InstanceShape,
    InviteLinkDomainRestriction, LinkSharingSettings, MaxPriorCycles, SecretRedactionSettings,
    SessionSharingPolicy, SharedNotebooksPolicy, SharedWorkflowsPolicy, SplitListSetting,
    TeamAiPermissionsSettings, TeamByoSettings, TeamLinkSharingSettings,
    TeamSecretRedactionSettings, TeamSettings, TelemetryDataCollectionPolicy, TelemetrySettings,
    Tier, UgcCollectionEnablementSetting, UgcCollectionSettings, UgcDataCollectionPolicy,
    UsageBasedPricingPolicy, UsageVisibilityGranularity, UsageVisibilityPolicy, WarpAiPolicy,
    Workspace, WorkspaceMember, WorkspaceMemberUsageInfo, WorkspaceSettings, WorkspaceSizePolicy,
};
use crate::auth::UserUid;
use crate::convert_to_server_experiment;
use crate::server::cloud_objects::listener::ObjectUpdateMessage;
use crate::server::experiments::ServerExperiment;
use crate::server::graphql::schema::object_action_history_from_gql;
use crate::server::ids::ServerId;
use crate::workspaces::workspace::{
    AiOverages, BonusGrantsPurchased, ByoApiKeyPolicy, ByoEndpointPolicy, CodebaseContextPolicy,
    EnterpriseCreditsAutoReloadPolicy, EnterprisePayAsYouGoPolicy, ManagedByokByoePolicy,
    MultiAdminPolicy, NativeWorkspacesPolicy, PurchaseAddOnCreditsPolicy,
    UsageBasedPricingSettings,
};

pub const PLACEHOLDER_WORKSPACE_UID: &str = "NOT_A_REAL_WORKSPACE_UID";

impl From<GqlTeamMember> for TeamMember {
    fn from(gql_team_member: GqlTeamMember) -> TeamMember {
        Self {
            uid: UserUid::new(&gql_team_member.uid.into_inner()),
            email: gql_team_member.email,
            role: gql_team_member.role.into(),
            is_disabled: gql_team_member.is_disabled,
        }
    }
}

/// Narrows a workspace to the teams the authenticated user actually belongs to.
///
/// The server hands workspace admins every team in the workspace so admin
/// surfaces can manage them, but a team the user is not a member of is not one
/// they can operate as in the client. Filtering here keeps every consumer of
/// `Workspace::teams` (team switcher, team spaces, warp drive teams, ...)
/// scoped to real memberships.
fn retain_authenticated_teams(workspace: &mut Workspace, user_uid: UserUid) {
    workspace
        .teams
        .retain(|team| team.members.iter().any(|member| member.uid == user_uid));
}

impl From<GqlManagedByokByoePolicy> for ManagedByokByoePolicy {
    fn from(gql_managed_byok_byoe_policy: GqlManagedByokByoePolicy) -> ManagedByokByoePolicy {
        Self {
            enabled: gql_managed_byok_byoe_policy.enabled,
        }
    }
}

impl From<GqlTeamByoSettings> for TeamByoSettings {
    fn from(gql_team_byo: GqlTeamByoSettings) -> TeamByoSettings {
        Self {
            first_party_enabled: gql_team_byo.first_party_enabled,
            endpoints_enabled: gql_team_byo.endpoints_enabled,
            allow_user_keys: gql_team_byo.allow_user_keys,
            allow_user_endpoints: gql_team_byo.allow_user_endpoints,
            first_party_keys: gql_team_byo
                .first_party_keys
                .into_iter()
                .map(From::from)
                .collect(),
            endpoints: gql_team_byo.endpoints.into_iter().map(From::from).collect(),
        }
    }
}

impl From<GqlByoFirstPartyKey> for ByoFirstPartyKey {
    fn from(gql_key: GqlByoFirstPartyKey) -> ByoFirstPartyKey {
        Self {
            credential_uid: gql_key.credential_uid.into_inner(),
        }
    }
}

impl From<GqlByoEndpointMetadata> for ByoEndpointMetadata {
    fn from(gql_endpoint: GqlByoEndpointMetadata) -> ByoEndpointMetadata {
        Self {
            uid: gql_endpoint.uid.into_inner(),
            name: gql_endpoint.name,
            enabled: gql_endpoint.enabled,
            credential_uid: gql_endpoint.credential_uid.into_inner(),
            models: gql_endpoint.models.into_iter().map(From::from).collect(),
        }
    }
}

impl From<GqlByoEndpointModelMetadata> for ByoEndpointModelMetadata {
    fn from(gql_model: GqlByoEndpointModelMetadata) -> ByoEndpointModelMetadata {
        Self {
            config_key: gql_model.config_key,
            slug: gql_model.slug,
            alias: gql_model.alias,
            display_name: gql_model.display_name,
            enabled: gql_model.enabled,
        }
    }
}

impl From<GqlMembershipRole> for MembershipRole {
    fn from(role: GqlMembershipRole) -> Self {
        match role {
            GqlMembershipRole::Owner => MembershipRole::Owner,
            GqlMembershipRole::Admin => MembershipRole::Admin,
            GqlMembershipRole::User => MembershipRole::User,
            GqlMembershipRole::Unknown => {
                report_error!(anyhow!(
                    "Invalid MembershipRole from server; treating as User"
                ));
                MembershipRole::User
            }
        }
    }
}

impl From<MembershipRole> for GqlMembershipRole {
    fn from(role: MembershipRole) -> Self {
        match role {
            MembershipRole::Owner => GqlMembershipRole::Owner,
            MembershipRole::Admin => GqlMembershipRole::Admin,
            MembershipRole::User => GqlMembershipRole::User,
        }
    }
}

impl From<GqlTeamVisibility> for TeamVisibility {
    fn from(visibility: GqlTeamVisibility) -> Self {
        match visibility {
            GqlTeamVisibility::Open => TeamVisibility::Open,
            GqlTeamVisibility::Private => TeamVisibility::Private,
            GqlTeamVisibility::Hidden => TeamVisibility::Hidden,
            GqlTeamVisibility::Other(value) => {
                report_error!(
                    "Invalid TeamVisibility from server; treating as Private",
                    extra: { "value" => %value },
                    warp_errors::ReportErrorLogMode::OncePerRun
                );
                // Fail closed: an unrecognized value must not be treated as Open,
                // since that would surface the invite-by-link control.
                TeamVisibility::Private
            }
        }
    }
}

impl From<GqlWorkspaceMemberUsageInfo> for WorkspaceMemberUsageInfo {
    fn from(
        gql_workspace_member_usage_info: GqlWorkspaceMemberUsageInfo,
    ) -> WorkspaceMemberUsageInfo {
        Self {
            request_limit: gql_workspace_member_usage_info.request_limit,
            requests_used_since_last_refresh: gql_workspace_member_usage_info
                .requests_used_since_last_refresh,
            is_unlimited: gql_workspace_member_usage_info.is_unlimited,
            is_request_limit_prorated: gql_workspace_member_usage_info.is_request_limit_prorated,
        }
    }
}

impl From<GqlWorkspaceMember> for WorkspaceMember {
    fn from(gql_workspace_member: GqlWorkspaceMember) -> WorkspaceMember {
        Self {
            uid: UserUid::new(&gql_workspace_member.uid.into_inner()),
            email: gql_workspace_member.email,
            role: gql_workspace_member.role.into(),
            is_disabled: gql_workspace_member.is_disabled,
            usage_info: gql_workspace_member.usage_info.into(),
        }
    }
}

impl From<GqlEmailInvite> for EmailInvite {
    fn from(gql_email_invite: GqlEmailInvite) -> EmailInvite {
        Self {
            invitee_email: gql_email_invite.email,
            expired: gql_email_invite.expired,
            team_uid: gql_email_invite
                .team_uid
                .map(|uid| ServerId::from_string_lossy(uid.into_inner())),
        }
    }
}

impl From<GqlInviteLinkDomainRestriction> for InviteLinkDomainRestriction {
    fn from(
        gql_invite_link_domain_restriction: GqlInviteLinkDomainRestriction,
    ) -> InviteLinkDomainRestriction {
        InviteLinkDomainRestriction {
            uid: ServerId::from_string_lossy(gql_invite_link_domain_restriction.uid.inner()),
            domain: gql_invite_link_domain_restriction.domain,
        }
    }
}

impl From<GqlWarpAiPolicy> for WarpAiPolicy {
    fn from(gql_warp_ai_policy: GqlWarpAiPolicy) -> WarpAiPolicy {
        Self {
            limit: i64::from(gql_warp_ai_policy.limit),
            is_code_suggestions_toggleable: gql_warp_ai_policy.is_code_suggestions_toggleable,
            is_prompt_suggestions_toggleable: gql_warp_ai_policy.is_prompt_suggestions_toggleable,
            is_next_command_enabled: gql_warp_ai_policy.is_next_command_enabled,
            is_git_operations_ai_enabled: gql_warp_ai_policy.is_git_operations_ai_enabled,
            is_voice_enabled: gql_warp_ai_policy.is_voice_enabled,
        }
    }
}

impl From<GqlTeamSizePolicy> for WorkspaceSizePolicy {
    fn from(gql_workspace_size_policy: GqlTeamSizePolicy) -> WorkspaceSizePolicy {
        Self {
            is_unlimited: gql_workspace_size_policy.is_unlimited,
            limit: i64::from(gql_workspace_size_policy.limit),
        }
    }
}

impl From<GqlSharedNotebooksPolicy> for SharedNotebooksPolicy {
    fn from(gql_shared_notebooks_policy: GqlSharedNotebooksPolicy) -> SharedNotebooksPolicy {
        Self {
            is_unlimited: gql_shared_notebooks_policy.is_unlimited,
            limit: i64::from(gql_shared_notebooks_policy.limit),
        }
    }
}

impl From<GqlSharedWorkflowsPolicy> for SharedWorkflowsPolicy {
    fn from(gql_shared_workflows_policy: GqlSharedWorkflowsPolicy) -> SharedWorkflowsPolicy {
        Self {
            is_unlimited: gql_shared_workflows_policy.is_unlimited,
            limit: i64::from(gql_shared_workflows_policy.limit),
        }
    }
}

impl From<GqlSessionSharingPolicy> for SessionSharingPolicy {
    fn from(gql_session_sharing_policy: GqlSessionSharingPolicy) -> SessionSharingPolicy {
        Self {
            is_enabled: gql_session_sharing_policy.enabled,
            max_session_size: u64::try_from(gql_session_sharing_policy.max_session_bytes_size)
                .unwrap_or_default(),
        }
    }
}

impl From<GqlAiAutonomyPolicy> for AIAutonomyPolicy {
    fn from(gql_ai_autonomy_policy: GqlAiAutonomyPolicy) -> AIAutonomyPolicy {
        Self {
            is_enabled: gql_ai_autonomy_policy.enabled,
            toggleable: gql_ai_autonomy_policy.toggleable,
        }
    }
}

impl From<GqlUgcCollectionEnablementSetting> for UgcCollectionEnablementSetting {
    fn from(
        gql_ugc_collection_enablement_setting: GqlUgcCollectionEnablementSetting,
    ) -> UgcCollectionEnablementSetting {
        match gql_ugc_collection_enablement_setting {
            GqlUgcCollectionEnablementSetting::Disable => UgcCollectionEnablementSetting::Disable,
            GqlUgcCollectionEnablementSetting::Enable => UgcCollectionEnablementSetting::Enable,
            GqlUgcCollectionEnablementSetting::RespectUserSetting => {
                UgcCollectionEnablementSetting::RespectUserSetting
            }
            GqlUgcCollectionEnablementSetting::Other(value) => {
                report_error!(
                    "Invalid UgcCollectionEnablementSetting. Make sure to update client GraphQL types!",
                    extra: { "value" => %value },
                    warp_errors::ReportErrorLogMode::OncePerRun
                );
                UgcCollectionEnablementSetting::RespectUserSetting
            }
        }
    }
}

impl From<GqlAdminEnablementSetting> for AdminEnablementSetting {
    fn from(gql_admin_enablement_setting: GqlAdminEnablementSetting) -> AdminEnablementSetting {
        match gql_admin_enablement_setting {
            GqlAdminEnablementSetting::Disable => AdminEnablementSetting::Disable,
            GqlAdminEnablementSetting::Enable => AdminEnablementSetting::Enable,
            GqlAdminEnablementSetting::RespectUserSetting => {
                AdminEnablementSetting::RespectUserSetting
            }
            GqlAdminEnablementSetting::Other(value) => {
                report_error!(
                    "Invalid AdminEnablementSetting. Make sure to update client GraphQL types!",
                    extra: { "value" => %value },
                    warp_errors::ReportErrorLogMode::OncePerRun
                );
                AdminEnablementSetting::RespectUserSetting
            }
        }
    }
}

impl From<&GqlAiPermissionsSettings> for AiPermissionsSettings {
    fn from(gql_ai_permissions_settings: &GqlAiPermissionsSettings) -> AiPermissionsSettings {
        Self {
            allow_ai_in_remote_sessions: gql_ai_permissions_settings.allow_ai_in_remote_sessions,
            remote_session_regex_list: compile_remote_session_regex_list(
                gql_ai_permissions_settings
                    .remote_session_regex_list
                    .clone(),
            ),
        }
    }
}

/// Compiles each remote-session command pattern into a [`Regex`], dropping (and reporting) any
/// pattern that fails to compile so one bad entry in an org's configuration cannot suppress the
/// rest of the list.
///
/// Throttled to once per run: an uncompilable pattern is a static configuration problem that
/// does not resolve itself between polls of the workspaces-metadata query, so reporting it every
/// time would page the same broken pattern at the poll rate for every affected user.
fn compile_remote_session_regex_list(patterns: impl IntoIterator<Item = String>) -> Vec<Regex> {
    patterns
        .into_iter()
        .filter_map(|pattern| match Regex::new(&pattern) {
            Ok(regex) => Some(regex),
            Err(_) => {
                report_error!(
                    "Invalid regex pattern for remote session detection",
                    extra: { "pattern" => %pattern },
                    warp_errors::ReportErrorLogMode::OncePerRun
                );
                None
            }
        })
        .collect()
}

impl From<GqlUgcDataCollectionPolicy> for UgcDataCollectionPolicy {
    fn from(gql_ugc_data_collection_policy: GqlUgcDataCollectionPolicy) -> UgcDataCollectionPolicy {
        Self {
            default_setting: UgcCollectionEnablementSetting::from(
                gql_ugc_data_collection_policy.default_setting,
            ),
            toggleable: gql_ugc_data_collection_policy.toggleable,
        }
    }
}

impl From<GqlTelemetryDataCollectionPolicy> for TelemetryDataCollectionPolicy {
    fn from(
        gql_telemetry_data_collection_policy: GqlTelemetryDataCollectionPolicy,
    ) -> TelemetryDataCollectionPolicy {
        Self {
            default: gql_telemetry_data_collection_policy.default,
            toggleable: gql_telemetry_data_collection_policy.toggleable,
        }
    }
}

impl From<GqlUsageBasedPricingPolicy> for UsageBasedPricingPolicy {
    fn from(gql_usage_based_pricing_policy: GqlUsageBasedPricingPolicy) -> UsageBasedPricingPolicy {
        Self {
            toggleable: gql_usage_based_pricing_policy.toggleable,
        }
    }
}

impl From<GqlAddonCreditsSettings> for AddonCreditsSettings {
    fn from(gql_settings: GqlAddonCreditsSettings) -> AddonCreditsSettings {
        Self {
            auto_reload_enabled: gql_settings.auto_reload_enabled,
            max_monthly_spend_cents: gql_settings.max_monthly_spend_cents,
            selected_auto_reload_credit_denomination: gql_settings
                .selected_auto_reload_credit_denomination,
        }
    }
}

impl From<GqlCodebaseContextPolicy> for CodebaseContextPolicy {
    fn from(gql_codebase_context_policy: GqlCodebaseContextPolicy) -> CodebaseContextPolicy {
        Self {
            toggleable: gql_codebase_context_policy.toggleable,
            index_limit: if gql_codebase_context_policy.is_unlimited_indices {
                None
            } else {
                Some(gql_codebase_context_policy.max_indices as u32)
            },
            max_files_per_repo: gql_codebase_context_policy.max_files_per_repo as u32,
        }
    }
}

impl From<GqlByoApiKeyPolicy> for ByoApiKeyPolicy {
    fn from(gql_byo_api_key_policy: GqlByoApiKeyPolicy) -> ByoApiKeyPolicy {
        Self {
            enabled: gql_byo_api_key_policy.enabled,
        }
    }
}

impl From<GqlByoEndpointPolicy> for ByoEndpointPolicy {
    fn from(gql_byo_endpoint_policy: GqlByoEndpointPolicy) -> ByoEndpointPolicy {
        Self {
            enabled: gql_byo_endpoint_policy.enabled,
        }
    }
}

impl From<GqlPurchaseAddOnCreditsPolicy> for PurchaseAddOnCreditsPolicy {
    fn from(
        gql_purchase_add_on_credits_policy: GqlPurchaseAddOnCreditsPolicy,
    ) -> PurchaseAddOnCreditsPolicy {
        Self {
            enabled: gql_purchase_add_on_credits_policy.enabled,
            premium_enabled: gql_purchase_add_on_credits_policy.premium_enabled,
            price_premium_bps: gql_purchase_add_on_credits_policy.price_premium_bps,
        }
    }
}

impl From<GqlEnterprisePayAsYouGoPolicy> for EnterprisePayAsYouGoPolicy {
    fn from(gql_policy: GqlEnterprisePayAsYouGoPolicy) -> EnterprisePayAsYouGoPolicy {
        Self {
            enabled: gql_policy.enabled,
        }
    }
}

impl From<GqlEnterpriseCreditsAutoReloadPolicy> for EnterpriseCreditsAutoReloadPolicy {
    fn from(gql_policy: GqlEnterpriseCreditsAutoReloadPolicy) -> EnterpriseCreditsAutoReloadPolicy {
        Self {
            enabled: gql_policy.enabled,
        }
    }
}

impl From<GqlMultiAdminPolicy> for MultiAdminPolicy {
    fn from(gql_policy: GqlMultiAdminPolicy) -> MultiAdminPolicy {
        Self {
            enabled: gql_policy.enabled,
        }
    }
}

impl From<GqlNativeWorkspacesPolicy> for NativeWorkspacesPolicy {
    fn from(gql_policy: GqlNativeWorkspacesPolicy) -> NativeWorkspacesPolicy {
        Self {
            enabled: gql_policy.enabled,
        }
    }
}

impl From<GqlInstanceShape> for InstanceShape {
    fn from(gql_instance_shape: GqlInstanceShape) -> InstanceShape {
        Self {
            vcpus: gql_instance_shape.vcpus,
            memory_gb: gql_instance_shape.memory_gb,
        }
    }
}

impl From<GqlAmbientAgentsPolicy> for AmbientAgentsPolicy {
    fn from(gql_policy: GqlAmbientAgentsPolicy) -> AmbientAgentsPolicy {
        Self {
            max_concurrent_agents: gql_policy.max_concurrent_agents,
            instance_shape: gql_policy.instance_shape.map(From::from),
        }
    }
}

impl From<GqlUsageVisibilityGranularity> for UsageVisibilityGranularity {
    fn from(gql_granularity: GqlUsageVisibilityGranularity) -> UsageVisibilityGranularity {
        match gql_granularity {
            GqlUsageVisibilityGranularity::OwnOnly => UsageVisibilityGranularity::OwnOnly,
            GqlUsageVisibilityGranularity::TeamAggregate => {
                UsageVisibilityGranularity::TeamAggregate
            }
            GqlUsageVisibilityGranularity::PerUserTotals => {
                UsageVisibilityGranularity::PerUserTotals
            }
            GqlUsageVisibilityGranularity::FullBreakdown => {
                UsageVisibilityGranularity::FullBreakdown
            }
            GqlUsageVisibilityGranularity::Other(value) => {
                report_error!(
                    "Invalid UsageVisibilityGranularity. Make sure to update client GraphQL types!",
                    extra: { "value" => %value },
                    warp_errors::ReportErrorLogMode::OncePerRun
                );
                // Fail closed to the most restrictive granularity.
                UsageVisibilityGranularity::OwnOnly
            }
        }
    }
}

fn from_gql_max_prior_cycles(value: i32) -> MaxPriorCycles {
    match value {
        0 => MaxPriorCycles::None,
        n if n > 0 => MaxPriorCycles::Limited(n as u32),
        -1 => MaxPriorCycles::Unlimited,
        other => {
            report_error!(
                "Unexpected maxPriorCycles value from server; treating as unlimited",
                extra: { "value" => %other }
            );
            MaxPriorCycles::None
        }
    }
}

impl From<GqlUsageVisibilityPolicy> for UsageVisibilityPolicy {
    fn from(gql_policy: GqlUsageVisibilityPolicy) -> UsageVisibilityPolicy {
        Self {
            admin_granularity: gql_policy.admin_granularity.into(),
            max_prior_cycles: from_gql_max_prior_cycles(gql_policy.max_prior_cycles),
        }
    }
}

fn convert_billing_cycle_usage(history: GqlBillingCycleUsageHistory) -> BillingCycleUsageData {
    BillingCycleUsageData {
        current_period_start: history.current_period_start.utc(),
        current_period_end: history.current_period_end.utc(),
        summaries: history
            .summaries
            .into_iter()
            .map(|summary| BillingCycleUsageSummary {
                period_start: summary.period_start.utc(),
                period_end: summary.period_end.utc(),
                entries: summary
                    .entries
                    .into_iter()
                    .map(|entry| BillingCycleUsageEntry {
                        subject_type: entry.subject_type,
                        subject_uid: entry.subject_uid,
                        subject_display_name: entry.subject_display_name,
                        cost_type: entry.cost_type,
                        usage_bucket: entry.usage_bucket,
                        usage_source: entry.usage_source,
                        credits_used: entry.credits_used,
                        cost_cents: entry.cost_cents,
                        attributed_team_uid: entry.attributed_team_uid,
                    })
                    .collect(),
            })
            .collect(),
    }
}

impl From<GqlTier> for Tier {
    fn from(gql_tier: GqlTier) -> Tier {
        Self {
            name: gql_tier.name,
            description: gql_tier.description,
            warp_ai_policy: gql_tier.warp_ai_policy.map(From::from),
            workspace_size_policy: gql_tier.team_size_policy.map(From::from),
            shared_notebooks_policy: gql_tier.shared_notebooks_policy.map(From::from),
            shared_workflows_policy: gql_tier.shared_workflows_policy.map(From::from),
            session_sharing_policy: gql_tier.session_sharing_policy.map(From::from),
            ai_autonomy_policy: gql_tier.ai_autonomy_policy.map(From::from),
            telemetry_data_collection_policy: gql_tier
                .telemetry_data_collection_policy
                .map(From::from),
            ugc_data_collection_policy: gql_tier.ugc_data_collection_policy.map(From::from),
            usage_based_pricing_policy: gql_tier.usage_based_pricing_policy.map(From::from),
            codebase_context_policy: gql_tier.codebase_context_policy.map(From::from),
            byo_api_key_policy: gql_tier.byo_api_key_policy.map(From::from),
            byo_endpoint_policy: gql_tier.byo_endpoint_policy.map(From::from),
            managed_byok_byoe_policy: gql_tier.managed_byok_byoe_policy.map(From::from),
            purchase_add_on_credits_policy: gql_tier.purchase_add_on_credits_policy.map(From::from),
            enterprise_pay_as_you_go_policy: gql_tier
                .enterprise_pay_as_you_go_policy
                .map(From::from),
            enterprise_credits_auto_reload_policy: gql_tier
                .enterprise_credits_auto_reload_policy
                .map(From::from),
            multi_admin_policy: gql_tier.multi_admin_policy.map(From::from),
            native_workspaces_policy: gql_tier.native_workspaces_policy.map(From::from),
            ambient_agents_policy: gql_tier.ambient_agents_policy.map(From::from),
            usage_visibility_policy: gql_tier.usage_visibility_policy.map(From::from),
        }
    }
}

impl From<GqlCustomerType> for CustomerType {
    fn from(gql_customer_type: GqlCustomerType) -> CustomerType {
        match gql_customer_type {
            GqlCustomerType::Free => CustomerType::Free,
            GqlCustomerType::Turbo => CustomerType::Turbo,
            GqlCustomerType::SelfServe => CustomerType::SelfServe,
            GqlCustomerType::Prosumer => CustomerType::Prosumer,
            GqlCustomerType::Legacy => CustomerType::Legacy,
            GqlCustomerType::Enterprise => CustomerType::Enterprise,
            GqlCustomerType::Business => CustomerType::Business,
            GqlCustomerType::Lightspeed => CustomerType::Lightspeed,
            GqlCustomerType::Build => CustomerType::Build,
            GqlCustomerType::BuildMax => CustomerType::BuildMax,
            GqlCustomerType::ProTrial | GqlCustomerType::TeamTrial | GqlCustomerType::Other(_) => {
                CustomerType::Unknown
            }
        }
    }
}

impl From<GqlDelinquencyStatus> for DelinquencyStatus {
    fn from(gql_delinquency_status: GqlDelinquencyStatus) -> DelinquencyStatus {
        match gql_delinquency_status {
            GqlDelinquencyStatus::NoDelinquency => DelinquencyStatus::NoDelinquency,
            GqlDelinquencyStatus::PastDue => DelinquencyStatus::PastDue,
            GqlDelinquencyStatus::Unpaid => DelinquencyStatus::Unpaid,
            GqlDelinquencyStatus::TeamLimitExceeded => DelinquencyStatus::TeamLimitExceeded,
            GqlDelinquencyStatus::Other(_) => DelinquencyStatus::Unknown,
        }
    }
}

impl From<GqlBillingMetadata> for BillingMetadata {
    fn from(gql_billing_metadata: GqlBillingMetadata) -> BillingMetadata {
        Self {
            tier: gql_billing_metadata.tier.into(),
            customer_type: gql_billing_metadata.customer_type.into(),
            delinquency_status: gql_billing_metadata.delinquency_status.into(),
            service_agreements: gql_billing_metadata.service_agreements,
            ai_overages: gql_billing_metadata.ai_overages.map(|overages| AiOverages {
                current_monthly_request_cost_cents: overages.current_monthly_request_cost_cents,
                current_monthly_requests_used: overages.current_monthly_requests_used,
                current_period_end: overages.current_period_end.utc(),
            }),
        }
    }
}

impl TryFrom<&BillingMetadata> for StripeSubscriptionPlan {
    type Error = ();

    fn try_from(billing_metadata: &BillingMetadata) -> Result<Self, Self::Error> {
        match billing_metadata.customer_type {
            CustomerType::Turbo => Ok(StripeSubscriptionPlan::Turbo),
            CustomerType::SelfServe => Ok(StripeSubscriptionPlan::Team),
            CustomerType::Prosumer => Ok(StripeSubscriptionPlan::Pro),
            CustomerType::Business => {
                // Check if this is a legacy Business Plan, or a new Build Business plan based on service agreement type
                // See: https://github.com/warpdotdev/warp-server/pull/6828#discussion_r2496242091
                match billing_metadata
                    .service_agreements
                    .first()
                    .map(|sa| sa.type_.clone())
                {
                    Some(ServiceAgreementType::SelfServe) => {
                        Ok(StripeSubscriptionPlan::BuildBusiness)
                    }
                    _ => Ok(StripeSubscriptionPlan::Business),
                }
            }
            CustomerType::Lightspeed => Ok(StripeSubscriptionPlan::Lightspeed),
            CustomerType::Build => Ok(StripeSubscriptionPlan::Build),
            CustomerType::BuildMax => Ok(StripeSubscriptionPlan::BuildMax),
            // legacy customer types we don't support anymore, or customer types that don't get billed via stripe
            CustomerType::Free
            | CustomerType::Legacy
            | CustomerType::Enterprise
            | CustomerType::Unknown => Err(()),
        }
    }
}

impl From<GqlWorkspaceSettings> for WorkspaceSettings {
    fn from(gql_workspace_settings: GqlWorkspaceSettings) -> WorkspaceSettings {
        Self {
            team_byo: gql_workspace_settings.team_byo.map(From::from),
            telemetry_settings: TelemetrySettings {
                force_enabled: gql_workspace_settings.telemetry_settings.force_enabled,
            },
            ugc_collection_settings: UgcCollectionSettings {
                setting: UgcCollectionEnablementSetting::from(
                    gql_workspace_settings.ugc_collection_settings.setting,
                ),
            },
            cloud_conversation_storage_settings: CloudConversationStorageSettings {
                setting: gql_workspace_settings
                    .cloud_conversation_storage_settings
                    .setting
                    .into(),
            },
            ai_permissions_settings: AiPermissionsSettings {
                allow_ai_in_remote_sessions: gql_workspace_settings
                    .ai_permissions_settings
                    .allow_ai_in_remote_sessions,
                remote_session_regex_list: compile_remote_session_regex_list(
                    gql_workspace_settings
                        .ai_permissions_settings
                        .remote_session_regex_list,
                ),
            },
            link_sharing_settings: LinkSharingSettings {
                anyone_with_link_sharing_enabled: gql_workspace_settings
                    .link_sharing_settings
                    .anyone_with_link_sharing_enabled,
                direct_link_sharing_enabled: gql_workspace_settings
                    .link_sharing_settings
                    .direct_link_sharing_enabled,
            },
            secret_redaction_settings: SecretRedactionSettings {
                enabled: gql_workspace_settings.secret_redaction_settings.enabled,
                regexes: gql_workspace_settings
                    .secret_redaction_settings
                    .regexes
                    .into_iter()
                    .map(|gql_regex| EnterpriseSecretRegex {
                        pattern: gql_regex.pattern,
                        name: gql_regex.name,
                    })
                    .collect(),
            },
            is_invite_link_enabled: gql_workspace_settings.is_invite_link_enabled,
            is_discoverable: gql_workspace_settings.is_discoverable,
            usage_based_pricing_settings: UsageBasedPricingSettings {
                enabled: gql_workspace_settings.usage_based_pricing_settings.enabled,
                max_monthly_spend_cents: gql_workspace_settings
                    .usage_based_pricing_settings
                    .max_monthly_spend_cents
                    .and_then(|cents| {
                        if cents < 0 {
                            report_error!(
                                "Usage-based pricing has a negative max monthly spend",
                                extra: { "cents" => %cents }
                            );
                            None
                        } else {
                            Some(cents as u32)
                        }
                    }),
            },
            addon_credits_settings: gql_workspace_settings.addon_credits_settings.into(),
            codebase_context_settings: CodebaseContextSettings {
                setting: gql_workspace_settings
                    .codebase_context_settings
                    .setting
                    .into(),
            },
            enable_warp_attribution: gql_workspace_settings
                .ambient_agent_settings
                .as_ref()
                .map(|s| s.enable_warp_attribution.clone().into())
                .unwrap_or_default(),
            default_host_slug: gql_workspace_settings
                .ambient_agent_settings
                .as_ref()
                .and_then(|s| s.default_host_slug.clone()),
        }
    }
}

impl From<GqlTeamSettings> for TeamSettings {
    fn from(gql_team_settings: GqlTeamSettings) -> TeamSettings {
        let map_regexes =
            |regexes: Vec<warp_graphql::workspace::SecretRedactionRegex>| -> Vec<EnterpriseSecretRegex> {
                regexes
                    .into_iter()
                    .map(|gql_regex| EnterpriseSecretRegex {
                        pattern: gql_regex.pattern,
                        name: gql_regex.name,
                    })
                    .collect()
            };
        Self {
            ugc_collection: EnforceableSetting {
                value: UgcCollectionEnablementSetting::from(gql_team_settings.ugc_collection.value),
                is_enforced_by_workspace: gql_team_settings.ugc_collection.is_enforced_by_workspace,
            },
            cloud_conversation_storage: EnforceableSetting {
                value: gql_team_settings.cloud_conversation_storage.value.into(),
                is_enforced_by_workspace: gql_team_settings
                    .cloud_conversation_storage
                    .is_enforced_by_workspace,
            },
            codebase_context: EnforceableSetting {
                value: gql_team_settings.codebase_context.value.into(),
                is_enforced_by_workspace: gql_team_settings
                    .codebase_context
                    .is_enforced_by_workspace,
            },
            ai_permissions: TeamAiPermissionsSettings {
                allow_ai_in_remote_sessions: EnforceableSetting {
                    value: gql_team_settings
                        .ai_permissions
                        .allow_ai_in_remote_sessions
                        .value,
                    is_enforced_by_workspace: gql_team_settings
                        .ai_permissions
                        .allow_ai_in_remote_sessions
                        .is_enforced_by_workspace,
                },
                remote_session_regex_list: compile_remote_session_regex_list(
                    gql_team_settings
                        .ai_permissions
                        .remote_session_regex_list
                        .values,
                ),
            },
            secret_redaction: TeamSecretRedactionSettings {
                enabled: EnforceableSetting {
                    value: gql_team_settings.secret_redaction.enabled.value,
                    is_enforced_by_workspace: gql_team_settings
                        .secret_redaction
                        .enabled
                        .is_enforced_by_workspace,
                },
                regexes: SplitListSetting {
                    values: map_regexes(gql_team_settings.secret_redaction.regexes.values),
                    workspace_entries: map_regexes(
                        gql_team_settings.secret_redaction.regexes.workspace_entries,
                    ),
                    team_entries: map_regexes(
                        gql_team_settings.secret_redaction.regexes.team_entries,
                    ),
                },
            },
            link_sharing: TeamLinkSharingSettings {
                anyone_with_link_sharing_enabled: EnforceableSetting {
                    value: gql_team_settings
                        .link_sharing
                        .anyone_with_link_sharing_enabled
                        .value,
                    is_enforced_by_workspace: gql_team_settings
                        .link_sharing
                        .anyone_with_link_sharing_enabled
                        .is_enforced_by_workspace,
                },
                direct_link_sharing_enabled: EnforceableSetting {
                    value: gql_team_settings
                        .link_sharing
                        .direct_link_sharing_enabled
                        .value,
                    is_enforced_by_workspace: gql_team_settings
                        .link_sharing
                        .direct_link_sharing_enabled
                        .is_enforced_by_workspace,
                },
            },
            telemetry_settings: TelemetrySettings {
                force_enabled: gql_team_settings.telemetry_settings.force_enabled,
            },
            usage_based_pricing_settings: UsageBasedPricingSettings {
                enabled: gql_team_settings.usage_based_pricing_settings.enabled,
                max_monthly_spend_cents: gql_team_settings
                    .usage_based_pricing_settings
                    .max_monthly_spend_cents
                    .and_then(|cents| {
                        if cents < 0 {
                            report_error!(
                                "Usage-based pricing has a negative max monthly spend",
                                extra: { "cents" => %cents }
                            );
                            None
                        } else {
                            Some(cents as u32)
                        }
                    }),
            },
            addon_credits_settings: gql_team_settings.addon_credits_settings.into(),
            enable_warp_attribution: gql_team_settings
                .ambient_agent_settings
                .as_ref()
                .map(|s| s.enable_warp_attribution.clone().into())
                .unwrap_or_default(),
            default_host_slug: gql_team_settings
                .ambient_agent_settings
                .as_ref()
                .and_then(|s| s.default_host_slug.clone()),
            team_byo: gql_team_settings.team_byo.map(From::from),
        }
    }
}

/// Derives a team's effective settings from the GraphQL payload. The settings
/// always come from the **team** payload (`gql_team.settings`), never from a
/// clone of the workspace settings. Workspace-scoped flags such as
/// discoverability are intentionally not part of `TeamSettings` and are read
/// from the workspace settings at their call sites.
///
/// Extracted from [`Team::from_gql`] so the team-payload sourcing is
/// unit-testable without constructing a full `GqlWorkspace`.
pub(crate) fn team_settings_from_gql(team_settings: GqlTeamSettings) -> TeamSettings {
    team_settings.into()
}

pub(crate) fn team_pending_email_invites_from_gql(
    workspace_pending_email_invites: &[GqlEmailInvite],
    team_uid: &cynic::Id,
) -> Vec<EmailInvite> {
    workspace_pending_email_invites
        .iter()
        .filter(|invite| invite.team_uid.as_ref() == Some(team_uid))
        .cloned()
        .map(Into::into)
        .collect()
}

impl Team {
    pub fn from_gql(gql_workspace: GqlWorkspace, gql_team: GqlTeam) -> Team {
        Self {
            uid: ServerId::from_string_lossy(gql_team.uid.inner()),
            name: gql_team.name.clone(),
            color: gql_team.color.clone(),
            members: gql_team
                .members
                .clone()
                .into_iter()
                .map(|gql_member| gql_member.into())
                .collect(),
            invite_link: gql_team.invite_link.clone(),
            pending_email_invites: team_pending_email_invites_from_gql(
                &gql_workspace.pending_email_invites,
                &gql_team.uid,
            ),
            invite_link_domain_restrictions: gql_workspace
                .invite_link_domain_restrictions
                .clone()
                .into_iter()
                .map(|gql_domain_restriction| gql_domain_restriction.into())
                .collect(),
            billing_metadata: gql_workspace.billing_metadata.clone().into(),
            stripe_customer_id: gql_workspace
                .stripe_customer_id
                .as_ref()
                .map(|id| id.clone().into_inner()),
            // Team-effective settings come from the team payload, not from a
            // clone of the workspace settings.
            settings: team_settings_from_gql(gql_team.settings),
            is_eligible_for_discovery: gql_workspace.is_eligible_for_discovery,
            has_billing_history: gql_workspace.has_billing_history,
            visibility: gql_team.visibility.into(),
        }
    }
}

impl From<GqlWorkspace> for Workspace {
    fn from(gql_workspace: GqlWorkspace) -> Workspace {
        Self {
            uid: ServerId::from_string_lossy(gql_workspace.uid.inner()).into(),
            name: gql_workspace.name.clone(),
            stripe_customer_id: gql_workspace
                .stripe_customer_id
                .as_ref()
                .map(|id| id.clone().into_inner()),
            teams: gql_workspace
                .teams
                .clone()
                .into_iter()
                .map(|gql_team| Team::from_gql(gql_workspace.clone(), gql_team))
                .collect(),
            billing_metadata: gql_workspace.billing_metadata.clone().into(),
            bonus_grants_purchased_this_month: gql_workspace
                .bonus_grants_info
                .spending_info
                .map(|info| BonusGrantsPurchased {
                    total_credits_purchased: info.current_month_credits_purchased,
                    cents_spent: info.current_month_spend_cents,
                })
                .unwrap_or_default(),
            billing_cycle_usage: gql_workspace
                .billing_cycle_usage_history
                .map(convert_billing_cycle_usage),
            has_billing_history: gql_workspace.has_billing_history,
            settings: gql_workspace.settings.clone().into(),
            invite_link_domain_restrictions: gql_workspace
                .invite_link_domain_restrictions
                .clone()
                .into_iter()
                .map(|gql_domain_restriction| gql_domain_restriction.into())
                .collect(),
            pending_email_invites: gql_workspace
                .pending_email_invites
                .clone()
                .into_iter()
                .map(|gql_email_invite| gql_email_invite.into())
                .collect(),
            is_eligible_for_discovery: gql_workspace.is_eligible_for_discovery,
            members: gql_workspace
                .members
                .clone()
                .into_iter()
                .map(|gql_member| gql_member.into())
                .collect(),
            total_requests_used_since_last_refresh: gql_workspace
                .total_requests_used_since_last_refresh,
        }
    }
}

impl From<GqlUser> for WorkspacesMetadataResponse {
    fn from(gql_user: GqlUser) -> WorkspacesMetadataResponse {
        let user_uid = UserUid::new(&gql_user.profile.uid);

        let workspaces: Vec<Workspace> = gql_user
            .workspaces
            .clone()
            .into_iter()
            .filter(|gql_workspace| {
                // TODO(skambashi): REV-717: Clean up this code once every user always has
                // a workspace, and the server no longer returns a placeholder workspace.
                gql_workspace.uid != PLACEHOLDER_WORKSPACE_UID.into()
            })
            .map(|gql_workspace| {
                let mut workspace = gql_workspace.into();
                retain_authenticated_teams(&mut workspace, user_uid);
                workspace
            })
            .collect();

        let joinable_teams = gql_user
            .discoverable_teams
            .clone()
            .into_iter()
            .map(|gql_joinable_team| gql_joinable_team.into())
            .collect();

        let experiments = gql_user
            .experiments
            .and_then(|experiments| convert_to_server_experiment!(experiments));

        // A teamless user's only workspace is the placeholder filtered out
        // above, so the user-level policy is the only place their add-on
        // credits purchase policy — gating and premium pricing alike —
        // survives (see
        // [`crate::workspaces::user_workspaces::UserWorkspaces::purchase_policy`]).
        let user_purchase_policy = gql_user
            .billing_metadata
            .and_then(|billing_metadata| billing_metadata.tier.purchase_add_on_credits_policy)
            .map(Into::into);

        // TODO(skambashi) refactor to return back workspaces, and not teams
        WorkspacesMetadataResponse {
            workspaces,
            joinable_teams,
            experiments,
            user_purchase_policy,
        }
    }
}

#[cfg(test)]
#[path = "gql_convert_tests.rs"]
mod tests;

pub fn object_update_message_from_gql(value: WarpDriveUpdate) -> Result<ObjectUpdateMessage> {
    match value {
        WarpDriveUpdate::ObjectActionOccurred(message) => {
            Ok(ObjectUpdateMessage::ObjectActionOccurred {
                history: object_action_history_from_gql(message.history)?,
            })
        }
        WarpDriveUpdate::ObjectContentUpdated(message) => {
            let server_object = message.object.try_into()?;
            let last_editor = message.last_editor.map(|e| e.into());
            Ok(ObjectUpdateMessage::ObjectContentChanged {
                server_object: Box::new(server_object),
                last_editor,
            })
        }
        WarpDriveUpdate::ObjectDeleted(message) => Ok(ObjectUpdateMessage::ObjectDeleted {
            object_uid: ServerId::from_string_lossy(message.object_uid.inner()),
        }),
        WarpDriveUpdate::ObjectMetadataUpdated(message) => {
            Ok(ObjectUpdateMessage::ObjectMetadataChanged {
                metadata: message.metadata.try_into()?,
            })
        }
        WarpDriveUpdate::ObjectPermissionsUpdated(message) => {
            Ok(ObjectUpdateMessage::ObjectPermissionsChangedV2 {
                object_uid: ServerId::from_string_lossy(message.object_uid.inner()),
                user_profiles: message
                    .user_profiles
                    .into_iter()
                    .flatten()
                    .map(Into::into)
                    .collect(),
                permissions: message.permissions.try_into()?,
            })
        }
        WarpDriveUpdate::TeamMembershipsChanged(_) => {
            Ok(ObjectUpdateMessage::TeamMembershipsChanged)
        }
        WarpDriveUpdate::AmbientTaskUpdated(message) => {
            Ok(ObjectUpdateMessage::AmbientTaskUpdated {
                task_id: message.task_id.inner().to_string(),
                timestamp: message.task_updated_ts.utc(),
            })
        }
        WarpDriveUpdate::Unknown => bail!("Unexpected WarpDriveUpdate variant"),
    }
}

impl From<GqlDiscoverableTeamData> for DiscoverableTeam {
    fn from(gql_discoverable_team: GqlDiscoverableTeamData) -> DiscoverableTeam {
        Self {
            team_uid: gql_discoverable_team.team_uid.into_inner(),
            num_members: i64::from(gql_discoverable_team.num_members),
            name: gql_discoverable_team.name,
            team_accepting_invites: gql_discoverable_team.team_accepting_invites,
        }
    }
}
