use structured_memory::{StructuredMemoryService, UpdateDocumentRequest};
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

// Helper function to create a document
// For non-root documents, we need to first create them via links from existing documents
async fn create_document(service: &StructuredMemoryService, id: Option<String>, content: &str) {
    match id {
        None => {
            // Root document - can update directly
            let request = UpdateDocumentRequest {
                id: None,
                content: content.to_string(),
            };
            let params = Parameters(request);
            service.update_document(params).await.unwrap();
        }
        Some(doc_id) => {
            // Non-root document - create via link first
            let root_content = format!("# Root Document\n\n[[{}]]", doc_id);
            let root_request = UpdateDocumentRequest {
                id: None,
                content: root_content,
            };
            let root_params = Parameters(root_request);
            service.update_document(root_params).await.unwrap();
            
            // Now update the created document with actual content
            let request = UpdateDocumentRequest {
                id: Some(doc_id),
                content: content.to_string(),
            };
            let params = Parameters(request);
            service.update_document(params).await.unwrap();
        }
    }
}

#[tokio::test]
async fn test_get_document_tree_no_args() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Call get_document_tree with no arguments
    let result = service.get_document_tree().await.unwrap();
    
    // Verify response format
    assert!(matches!(result, CallToolResult { .. }));
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.content.len(), 1);
    
    // Should return tree structure even with just root document
    if let Some(content) = result.content.first() {
        let yaml_str = &content.as_text().unwrap().text;
        assert_eq!(yaml_str, "root\n");
    }
}

#[tokio::test] 
async fn test_yaml_format_valid() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Create a simple document structure
    create_document(&service, Some("parent".to_string()), "# Parent\n\n[[child1]] [[child2]]").await;
    
    let result = service.get_document_tree().await.unwrap();
    
    if let Some(content) = result.content.first() {
        let yaml_str = &content.as_text().unwrap().text;
        let expected_yaml = "root:\n- parent:\n  - child1\n  - child2\n";
        assert_eq!(yaml_str, expected_yaml);
    }
}

#[tokio::test]
async fn test_tree_hierarchy_correct() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Create a simple hierarchical structure to test tree correctness
    // Just test root -> child1 -> [grandchild1, grandchild2]
    create_document(&service, None, "# Root\n\n[[child1]]").await;
    create_document(&service, Some("child1".to_string()), "# Child1\n\n[[grandchild1]] [[grandchild2]]").await;
    
    let result = service.get_document_tree().await.unwrap();
    
    if let Some(content) = result.content.first() {
        let yaml_str = &content.as_text().unwrap().text;
        let expected_yaml = "root:\n- child1:\n  - grandchild1\n  - grandchild2\n";
        assert_eq!(yaml_str, expected_yaml);
    }
}

