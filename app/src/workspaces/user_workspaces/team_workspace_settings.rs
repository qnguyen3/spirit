//! Team-scoped reads of workspace settings, plus the [`TeamScope`] types that name which team a
//! read is for.
//!
//! Two layers gate what a member may do, at different granularities:
//! - Plan entitlements are **workspace-level**: billing metadata is workspace-owned, so they hold
//!   regardless of which team a window has selected, and read `current_workspace()` (e.g.
//!   [`UserWorkspaces::is_managed_byok_byoe_enabled`]).
//! - Admin policies are **team-scoped**: they narrow an entitlement for one team, so they take a
//!   [`TeamScope`] and read that team (e.g. [`UserWorkspaces::team_byo_for_scope`]).
//!
//! A team-scoped policy only bites once its workspace-level entitlement turns it on -- a plan that
//! does not manage credentials centrally has no `team_byo` to enforce, so members fall back to the
//! plan's own BYO entitlement.

use std::rc::Rc;

use regex::Regex;
#[cfg(test)]
use warpui::WindowId;
use warpui::{AppContext, Entity, SingletonEntity, WeakViewHandle};

use super::UserWorkspaces;
use crate::server::ids::ServerId;
use crate::workspaces::team::Team;
use crate::workspaces::workspace::{TeamByoSettings, Workspace};

mod sealed {
    pub trait Sealed {}
}

/// Reads a [`TeamContext`]'s team.
///
/// A [`TeamContext`] is the "key" external modules use to obtain a team-level setting. The
/// only way external modules can obtain this "key" is by exchanging a ViewContext or a
/// ViewHandle for one. Once minted, a [`TeamContext`] cannot be copied, cloned, or moved.
/// This ensures that the external operations which need TeamScopes (i.e. to exchange for a
/// team setting) is scoped to the view (and therefore team-scoped window) that started the
/// operation. External callers shouldn't copy a TeamContext to a Singleton model for example,
/// risking leaking that TeamContext / team info to a different window with another team.
///
/// Sealed: only this module implements [`sealed::Sealed`], so a scope can never be minted
/// outside [`UserWorkspaces`].
#[allow(private_bounds)]
pub trait TeamScope: sealed::Sealed {
    fn team_uid(&self) -> Option<ServerId>;
}

/// The team a view renders as, borrowed for the duration of a single read.
///
/// It is resolved at the point of use so policy reads follow the view between windows.
pub struct TeamContext<'a> {
    team_uid: Option<&'a ServerId>,
}

impl sealed::Sealed for TeamContext<'_> {}

impl TeamScope for TeamContext<'_> {
    fn team_uid(&self) -> Option<ServerId> {
        self.team_uid.copied()
    }
}

/// Resolves a [`TeamContext`] on demand from a view captured up front. See
/// [`UserWorkspaces::team_context_resolver`].
pub type TeamContextResolver = Rc<dyn for<'a> Fn(&'a AppContext) -> TeamContext<'a>>;

impl UserWorkspaces {
    pub(crate) fn team_context<'a, T: Entity>(
        &'a self,
        view: &WeakViewHandle<T>,
        app: &AppContext,
    ) -> TeamContext<'a> {
        let team_uid = self.team_for_view_handle(view, app).map(|team| &team.uid);
        TeamContext { team_uid }
    }

    /// Captures `view` as a reusable source of [`TeamContext`], for consumers that cannot name
    /// a view at the boundaries where they need one.
    pub fn team_context_resolver<T: Entity>(view: WeakViewHandle<T>) -> TeamContextResolver {
        Rc::new(move |app| Self::as_ref(app).team_context(&view, app))
    }

    /// A resolver for tests that build a model without a window to resolve against.
    #[cfg(any(test, feature = "test-util"))]
    pub fn teamless_context_resolver_for_test() -> TeamContextResolver {
        Rc::new(|_| TeamContext { team_uid: None })
    }

    /// A [`TeamContext`] for a bare window, for tests that build scopes without standing up a
    /// view for each one. Production exchanges a view or a view context for a scope; this is
    /// `#[cfg(test)]` precisely so that contract holds.
    #[cfg(test)]
    pub(crate) fn team_context_for_window_for_test(&self, window_id: WindowId) -> TeamContext<'_> {
        TeamContext {
            team_uid: self
                .team_uid_for_window(window_id)
                .and_then(|team_uid| self.team_from_uid(team_uid))
                .map(|team| &team.uid),
        }
    }

    /// The team a scope names, when it names one that is still in the current workspace.
    ///
    /// Deliberately private. Callers get a resolved *setting* from a getter that takes their
    /// scope, never a `&Team` they could carry somewhere the scope never reached. Wanting a
    /// `&Team` at a call site means the read belongs behind a new getter here instead.
    fn team_from_scope<S: TeamScope + ?Sized>(&self, scope: &S) -> Option<&Team> {
        scope
            .team_uid()
            .and_then(|team_uid| self.team_from_uid(team_uid))
    }

    /// Whether `scope`'s team admins allows its members to use their own provider API keys.
    ///
    /// Without the managed BYOK/BYOE policy there is no team-level restriction, so this returns
    /// true and the normal BYO entitlement applies.
    pub fn are_member_byo_keys_allowed<S: TeamScope + ?Sized>(&self, scope: &S) -> bool {
        !self.is_managed_byok_byoe_enabled()
            || self
                .team_byo_for_scope(scope)
                .is_some_and(|team_byo| team_byo.first_party_enabled && team_byo.allow_user_keys)
    }

    /// [`Self::are_member_byo_endpoints_allowed`] across every team at once, for callers with no
    /// window: id resolution and preference reconciliation act on state that follows the user
    /// between teams and devices, so neither may turn on one arbitrarily elected team's policy.
    pub fn is_byo_endpoint_enabled_for_any_team(&self, app: &AppContext) -> bool {
        self.is_byo_endpoint_enabled(app) && self.any_team_allows_member_byo_endpoints()
    }

    /// Unlike [`Self::team_byo_for_scope`], several teams is not ambiguous here: any one
    /// allowing is enough. No teams still falls back to the workspace's own policy.
    fn any_team_allows_member_byo_endpoints(&self) -> bool {
        if !self.is_managed_byok_byoe_enabled() {
            return true;
        }
        fn allows(team_byo: &TeamByoSettings) -> bool {
            team_byo.endpoints_enabled && team_byo.allow_user_endpoints
        }
        let mut teams = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.teams.iter())
            .peekable();
        if teams.peek().is_none() {
            return self
                .current_workspace()
                .and_then(|workspace| workspace.settings.team_byo.as_ref())
                .is_some_and(allows);
        }
        teams.any(|team| team.settings.team_byo.as_ref().is_some_and(allows))
    }

    /// Resolves a per-team setting for `scope`: the scope's own team when it names one, otherwise
    /// `current_workspace().settings`.
    ///
    /// A scope naming an unresolvable team yields `absent`, never another team's value. The
    /// no-team branch reads `current_workspace().settings` unconditionally; for a member on teams
    /// that is the server's arbitrarily-elected stand-in (see [`TeamScope`]), a deliberate
    /// simplification because a windowed terminal is never expected to present a teamless scope,
    /// so in practice only a genuinely teamless user reaches it, whose workspace settings the
    /// server computes from tier defaults.
    fn scoped_or_workspace_setting<'a, S: TeamScope + ?Sized, T>(
        &'a self,
        scope: &S,
        from_team: impl FnOnce(&'a Team) -> T,
        from_workspace: impl FnOnce(&'a Workspace) -> T,
        absent: T,
    ) -> T {
        match scope.team_uid() {
            Some(_) => self.team_from_scope(scope).map_or(absent, from_team),
            None => self.current_workspace().map_or(absent, from_workspace),
        }
    }

    /// The `team_byo` policy that governs `scope`. See [`Self::scoped_or_workspace_setting`] for
    /// the no-team fallback.
    fn team_byo_for_scope<S: TeamScope + ?Sized>(&self, scope: &S) -> Option<&TeamByoSettings> {
        self.scoped_or_workspace_setting(
            scope,
            |team| team.settings.team_byo.as_ref(),
            |workspace| workspace.settings.team_byo.as_ref(),
            None,
        )
    }

    /// The remote-session command patterns configured by `scope`'s team. See
    /// [`Self::scoped_or_workspace_setting`] for the no-team fallback.
    pub(crate) fn get_remote_session_regex_list<S: TeamScope + ?Sized>(
        &self,
        scope: &S,
    ) -> &[Regex] {
        self.scoped_or_workspace_setting(
            scope,
            |team| {
                team.settings
                    .ai_permissions
                    .remote_session_regex_list
                    .as_slice()
            },
            |workspace| {
                workspace
                    .settings
                    .ai_permissions_settings
                    .remote_session_regex_list
                    .as_slice()
            },
            &[],
        )
    }
}
