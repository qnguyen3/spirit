#[cfg(not(target_family = "wasm"))]
pub mod persistence;

use anyhow::Result;
use cloud_objects::cloud_object::{
    GenericCloudObject, GenericServerObject, ObjectType, ServerObjectModel,
};
use cloud_objects::ids::{ServerId, SyncId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque identifier of the server-side document a notebook was generated from.
///
/// The wire format is a UUID string; it is parsed and re-rendered verbatim so
/// existing rows and server payloads keep round-tripping unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NotebookDocumentId(Uuid);

impl std::fmt::Display for NotebookDocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&str> for NotebookDocumentId {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        Ok(Self(Uuid::try_parse(value)?))
    }
}

impl TryFrom<String> for NotebookDocumentId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        Self::try_from(value.as_str())
    }
}

/// Serialized representation of a notebook for sync queue
/// The document id and conversation id are stored here to avoid polluting the
/// generic CreateObjectRequest type.
#[derive(Serialize, Deserialize)]
pub struct SerializedNotebook {
    pub data: String,
    pub ai_document_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudNotebookModel {
    pub title: String,
    pub data: String,
    pub ai_document_id: Option<NotebookDocumentId>,
    /// This is the server-generated conversation token, not a client-side conversation id.
    pub conversation_id: Option<String>,
}

impl ServerObjectModel for CloudNotebookModel {
    fn object_type(&self) -> ObjectType {
        ObjectType::Notebook
    }
}

/// This is the notebook_id in the database associated with this notebook.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct NotebookId(ServerId);
cloud_objects::server_id_traits! { NotebookId, "Notebook" }

impl From<NotebookId> for SyncId {
    fn from(id: NotebookId) -> Self {
        Self::ServerId(id.into())
    }
}

/// `CloudNotebook` is a notebook retrieved from the server.
pub type CloudNotebook = GenericCloudObject<NotebookId, CloudNotebookModel>;
pub type ServerNotebook = GenericServerObject<NotebookId, CloudNotebookModel>;
