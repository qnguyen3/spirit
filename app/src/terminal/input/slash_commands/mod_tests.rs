use super::{SlashCommandSelectionBehavior, slash_command_selection_behavior};
use crate::search::slash_command_menu::static_commands::commands;

#[test]
fn selection_behavior_inserts_text_for_required_argument_commands() {
    assert_eq!(
        slash_command_selection_behavior(&commands::RENAME_TAB),
        SlashCommandSelectionBehavior::InsertCommandText("/rename-tab ".to_owned())
    );
    let argument = commands::RENAME_TAB
        .argument
        .as_ref()
        .expect("rename-tab should require an argument");
    assert!(!argument.is_optional);
    assert_eq!(argument.hint_text, Some("<tab name>"));
}

#[test]
fn selection_behavior_executes_argumentless_and_execute_on_selection_commands() {
    assert_eq!(
        slash_command_selection_behavior(&commands::CHANGELOG),
        SlashCommandSelectionBehavior::Execute
    );
    assert_eq!(
        slash_command_selection_behavior(&commands::FEEDBACK),
        SlashCommandSelectionBehavior::Execute
    );
}

#[cfg(all(feature = "local_fs", windows))]
mod windows {
    use std::sync::Arc;

    use super::super::*;
    use crate::terminal::ShellLaunchData;
    use crate::terminal::model::session::SessionInfo;
    use crate::terminal::model::session::command_executor::testing::TestCommandExecutor;
    use crate::terminal::shell::ShellType;

    fn wsl_session() -> Session {
        Session::new(
            SessionInfo::new_for_test().with_shell_type(ShellType::Bash),
            Arc::new(TestCommandExecutor::default()),
        )
        .with_shell_launch_data(ShellLaunchData::WSL {
            distro: "Ubuntu".to_owned(),
        })
    }

    #[test]
    fn open_file_command_converts_wsl_paths_to_host_paths() {
        let session = wsl_session();
        let cases = [
            (
                "/home/ubuntu",
                "subdir/test.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\test.txt",
                None,
            ),
            (
                "/home/ubuntu/project",
                "../test.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\test.txt",
                None,
            ),
            (
                "/home/ubuntu",
                "subdir/file\\ name.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\file name.txt",
                None,
            ),
            (
                "/home/ubuntu",
                "subdir/test.txt:4:2",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\test.txt",
                Some(LineAndColumnArg {
                    line_num: 4,
                    column_num: Some(2),
                }),
            ),
        ];

        for (current_dir, raw_arg, expected_path, expected_line_col) in cases {
            let (path, line_col) = open_file_command_path(&session, current_dir, raw_arg);

            assert_eq!(path, PathBuf::from(expected_path));
            assert_eq!(line_col, expected_line_col);
        }
    }
}
