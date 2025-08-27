use structured_memory::{StructuredMemoryService, ReadDocumentRequest};
use rmcp::{
    handler::server::tool::Parameters,
    model::CallToolResult,
};
use tempfile::TempDir;

// Helper function to setup service with temporary directory
async fn setup_temp_service() -> (StructuredMemoryService, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_str().unwrap().to_string();
    let service = StructuredMemoryService::new(data_dir);
    (service, temp_dir)
}

#[tokio::test]
async fn test_read_root_document_default() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Create request without id (should default to root)
    let request = ReadDocumentRequest { id: None };
    let params = Parameters(request);
    
    // Call read_document
    let result = service.read_document(params).await.unwrap();
    
    // Verify response format
    assert!(matches!(result, CallToolResult { .. }));
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.content.len(), 1);
    
    // Extract content and verify it's the root document
    if let Some(content) = result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        let expected_content = "# Root Document\n\nThis is the root document of your structured memory.\n";
        assert_eq!(content_str, expected_content);
    }
}

#[tokio::test]
async fn test_read_specific_document() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // First create a test document via root link
    let test_content = "# Test Document\n\nThis is a test document.";
    
    // Create link from root document
    let root_content = format!("# Root Document\n\n[[test_doc]]");
    let root_request = structured_memory::UpdateDocumentRequest {
        id: None,
        content: root_content,
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    // Now update the created document with actual content
    let update_request = structured_memory::UpdateDocumentRequest {
        id: Some("test_doc".to_string()),
        content: test_content.to_string(),
    };
    let update_params = Parameters(update_request);
    service.update_document(update_params).await.unwrap();
    
    // Now read the specific document
    let read_request = ReadDocumentRequest { 
        id: Some("test_doc".to_string()) 
    };
    let read_params = Parameters(read_request);
    
    let result = service.read_document(read_params).await.unwrap();
    
    // Verify response
    assert_eq!(result.is_error, Some(false));
    if let Some(content) = result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        assert_eq!(content_str, test_content);
    }
}

#[tokio::test]
async fn test_read_nonexistent_document_error() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Try to read non-existent document
    let request = ReadDocumentRequest { 
        id: Some("nonexistent".to_string()) 
    };
    let params = Parameters(request);
    
    // Should return error
    let result = service.read_document(params).await;
    
    assert!(result.is_err());
    
    // Verify error message matches specification exactly
    let error = result.unwrap_err();
    assert_eq!(error.message, "Error: id: not found");
}

#[tokio::test]
async fn test_read_document_markdown_format() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Create document with markdown content via root link
    let markdown_content = "# Heading 1\n\n## Heading 2\n\n- List item 1\n- List item 2\n\n**Bold text**";
    
    // Create link from root document  
    let root_content = format!("# Root Document\n\n[[markdown_test]]");
    let root_request = structured_memory::UpdateDocumentRequest {
        id: None,
        content: root_content,
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    // Now update the created document with actual content
    let update_request = structured_memory::UpdateDocumentRequest {
        id: Some("markdown_test".to_string()),
        content: markdown_content.to_string(),
    };
    let update_params = Parameters(update_request);
    service.update_document(update_params).await.unwrap();
    
    // Read document
    let read_request = ReadDocumentRequest { 
        id: Some("markdown_test".to_string()) 
    };
    let read_params = Parameters(read_request);
    
    let result = service.read_document(read_params).await.unwrap();
    
    // Verify markdown format preserved
    if let Some(content) = result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        assert_eq!(content_str, markdown_content);
    }
}