//! File attachment and upload types.

use serde::{Deserialize, Serialize};

/// The kind of an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// An image attachment.
    Image,
    /// A generic file attachment.
    File,
    /// A video attachment.
    Video,
    /// An audio attachment.
    Audio,
}

/// A file attachment received from a chat adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Optional human-readable name.
    pub name: Option<String>,
    /// MIME type such as `image/png`.
    pub mime_type: Option<String>,
    /// Size in bytes, if known.
    pub size: Option<u64>,
    /// URL where the attachment can be fetched.
    pub url: Option<String>,
    /// Raw bytes of the attachment, if already downloaded.
    #[serde(skip)]
    pub data: Option<bytes::Bytes>,
    /// The kind of attachment.
    pub kind: AttachmentKind,
}

/// A file upload to be sent along with a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUpload {
    /// The filename to present to the recipient.
    pub filename: String,
    /// MIME type of the file.
    pub mime_type: Option<String>,
    /// Raw file bytes.
    #[serde(skip)]
    pub data: bytes::Bytes,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_kind_roundtrip() {
        let json = serde_json::to_string(&AttachmentKind::Image).expect("serialize");
        let back: AttachmentKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, AttachmentKind::Image);
    }

    #[test]
    fn attachment_debug() {
        let att = Attachment {
            name: Some("photo.png".into()),
            mime_type: Some("image/png".into()),
            size: Some(1024),
            url: None,
            data: None,
            kind: AttachmentKind::Image,
        };
        let dbg = format!("{att:?}");
        assert!(dbg.contains("photo.png"));
    }

    #[test]
    fn file_upload_debug() {
        let fu = FileUpload {
            filename: "report.pdf".into(),
            mime_type: Some("application/pdf".into()),
            data: bytes::Bytes::from_static(b"fake"),
        };
        let dbg = format!("{fu:?}");
        assert!(dbg.contains("report.pdf"));
    }
}
