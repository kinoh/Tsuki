pub mod service;

pub use service::StructuredMemoryService;

// Re-export types needed for testing
pub use service::{ReadDocumentRequest, UpdateDocumentRequest};
