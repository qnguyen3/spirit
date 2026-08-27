use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use warp_editor::render::model::LineCount;
use warp_multi_agent_api::{BaseRef, CurrentRef, base_ref, current_ref, diff_hunk};

use crate::code_review::comments::{
    AttachedReviewComment as CodeReviewComment, ReviewCommentBatch,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurrentHead {
    BranchName(String),
    HeadlessCommitSha(String),
}

impl CurrentHead {
    pub fn title(&self) -> String {
        match self {
            CurrentHead::BranchName(name) => name.clone(),
            CurrentHead::HeadlessCommitSha(sha) => {
                let short = sha.chars().take(7).collect::<String>();
                format!("Commit {short}")
            }
        }
    }
}

impl From<CurrentHead> for CurrentRef {
    fn from(value: CurrentHead) -> Self {
        Self {
            r#ref: Some(match value {
                CurrentHead::BranchName(name) => current_ref::Ref::BranchName(name),
                CurrentHead::HeadlessCommitSha(sha) => current_ref::Ref::HeadlessCommitSha(sha),
            }),
        }
    }
}

impl From<CurrentHead> for diff_hunk::Current {
    fn from(value: CurrentHead) -> Self {
        match value {
            CurrentHead::BranchName(name) => diff_hunk::Current::CurrentBranchName(name),
            CurrentHead::HeadlessCommitSha(sha) => {
                diff_hunk::Current::CurrentHeadlessCommitSha(sha)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffBase {
    BranchName(String),
    HeadlessCommitSha(String),
    UncommittedChanges,
}

impl From<DiffBase> for BaseRef {
    fn from(value: DiffBase) -> Self {
        Self {
            r#ref: Some(match value {
                DiffBase::BranchName(name) => base_ref::Ref::BranchName(name),
                DiffBase::HeadlessCommitSha(sha) => base_ref::Ref::HeadlessCommitSha(sha),
                DiffBase::UncommittedChanges => base_ref::Ref::UncommittedChanges(()),
            }),
        }
    }
}

impl From<DiffBase> for diff_hunk::Base {
    fn from(value: DiffBase) -> Self {
        match value {
            DiffBase::BranchName(branch_name) => diff_hunk::Base::BaseBranchName(branch_name),
            DiffBase::HeadlessCommitSha(sha) => diff_hunk::Base::BaseHeadlessCommitSha(sha),
            DiffBase::UncommittedChanges => diff_hunk::Base::UncommittedChanges(()),
        }
    }
}

/// A simplified diff hunk for use in DiffSet attachments
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSetHunk {
    pub line_range: Range<LineCount>,
    pub diff_content: String,
    pub lines_added: u32,
    pub lines_removed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentReviewCommentBatch {
    /// The review comments in this batch. Uses `code_review::comments::ReviewComment`
    /// because it contains full target information needed for API conversion and UI rendering.
    pub comments: Vec<CodeReviewComment>,
    /// All diff hunks that have comments in this batch attached to them, grouped by file name.
    pub diff_set: HashMap<String, Vec<DiffSetHunk>>,
}

impl AgentReviewCommentBatch {
    pub fn review_comments(&self) -> ReviewCommentBatch {
        ReviewCommentBatch::from_comments(self.comments.clone())
    }
}
