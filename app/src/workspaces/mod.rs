pub mod gql_convert;
pub mod team;
pub mod team_tester;
pub mod update_manager;
pub mod user_profiles;
pub mod user_workspaces;
pub mod workspace;

/// The drive a cloud object belongs to.
///
/// Only teams still consume this; it goes away with the Teams surface.
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Space {
    /// The current user's personal drive.
    #[default]
    Personal,
    /// A team that the current user belongs to.
    Team { team_uid: crate::server::ids::ServerId },
    /// An object shared from a drive the user is not a member of.
    Shared,
}
