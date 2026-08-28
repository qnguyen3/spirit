use std::ops::Range;

/// A review comment imported from a provider (e.g. GitHub) for display in the
/// code review panel.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertReviewComment {
    pub comment_id: String,
    pub author: String,
    pub last_modified_timestamp: String,
    pub comment_body: String,
    pub parent_comment_id: Option<String>,
    /// The file and line range the comment is attached to.
    /// If None, the comment applies to the whole diff set.
    pub comment_location: Option<InsertedCommentLocation>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertedCommentLocation {
    /// Repo-relative path of the file the comment is attached to.
    pub relative_file_path: String,
    /// The specific line range the comment is attached to.
    /// If None, the comment applies to the whole file.
    pub line: Option<InsertedCommentLine>,
}

/// The side of a diff that a comment is attached to.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CommentSide {
    /// The right side of the diff (new file / additions).
    Right,
    /// The left side of the diff (old file / deletions).
    Left,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertedCommentLine {
    pub comment_line_range: Range<usize>,
    /// The diff hunk line range overlaps with the comment line range
    /// but may not match it exactly. We need this in order to be able
    /// to find the full diff hunk this comment is attached to.
    pub diff_hunk_line_range: Range<usize>,
    /// The diff hunk text is needed to find where to attach comments
    /// when line numbers on the local and remote branches have diverged.
    pub diff_hunk_text: String,
    /// The side of the diff the comment is attached to.
    pub side: Option<CommentSide>,
}
