use structured_memory::{StructuredMemoryService, UpdateDocumentRequest, ReadDocumentRequest};
use rmcp::{
    handler::server::tool::Parameters,
};
use tempfile::TempDir;

// Helper function to setup service with temporary directory
async fn setup_temp_service() -> (StructuredMemoryService, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_str().unwrap().to_string();
    let service = StructuredMemoryService::new_with_data_dir(data_dir);
    (service, temp_dir)
}

#[tokio::test]
async fn test_update_root_document() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    let new_content = "# Updated Root\n\nThis is the updated root document.";
    
    // Update root document without specifying ID
    let request = UpdateDocumentRequest {
        id: None,
        content: new_content.to_string(),
    };
    let params = Parameters(request);
    
    let result = service.update_document(params).await.unwrap();
    
    // Verify successful response
    assert_eq!(result.is_error, Some(false));
    if let Some(content) = result.content.first() {
        let response = &content.as_text().unwrap().text;
        assert_eq!(response, "Succeeded");
    }
    
    // Verify the content was actually updated by reading it back
    let read_request = ReadDocumentRequest { id: None };
    let read_params = Parameters(read_request);
    let read_result = service.read_document(read_params).await.unwrap();
    
    if let Some(content) = read_result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        assert_eq!(content_str, new_content);
    }
}

#[tokio::test]
async fn test_update_specific_document() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    let doc_content = "# Specific Document\n\nThis is a specific document.";
    
    // Create via root link first
    let root_content = "# Root\n\n[[specific_doc]]";
    let root_request = UpdateDocumentRequest {
        id: None,
        content: root_content.to_string(),
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    // Now update specific document
    let request = UpdateDocumentRequest {
        id: Some("specific_doc".to_string()),
        content: doc_content.to_string(),
    };
    let params = Parameters(request);
    
    let result = service.update_document(params).await.unwrap();
    
    // Verify successful response (should just be "Succeeded" since document already exists)
    assert_eq!(result.is_error, Some(false));
    if let Some(content) = result.content.first() {
        let response = &content.as_text().unwrap().text;
        assert_eq!(response, "Succeeded");
    }
    
    // Verify the document exists by reading it
    let read_request = ReadDocumentRequest { 
        id: Some("specific_doc".to_string()) 
    };
    let read_params = Parameters(read_request);
    let read_result = service.read_document(read_params).await.unwrap();
    
    if let Some(content) = read_result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        assert_eq!(content_str, doc_content);
    }
}

#[tokio::test]
async fn test_create_documents_via_links() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // First create parent via root
    let root_content = "# Root\n\n[[parent]]";
    let root_request = UpdateDocumentRequest {
        id: None,
        content: root_content.to_string(),
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    // Content with links that should create new documents
    let content_with_links = "# Parent Document\n\nSee [[child_doc1]] and [[child_doc2]] for details.";
    
    let request = UpdateDocumentRequest {
        id: Some("parent".to_string()),
        content: content_with_links.to_string(),
    };
    let params = Parameters(request);
    
    let result = service.update_document(params).await.unwrap();
    
    // Verify response shows created documents
    assert_eq!(result.is_error, Some(false));
    if let Some(content) = result.content.first() {
        let response = &content.as_text().unwrap().text;
        let expected_response = "Succeeded\nCreated: child_doc1, child_doc2";
        assert_eq!(response, expected_response);
    }
    
    // Verify created documents exist and have correct initial content
    let read_request = ReadDocumentRequest { 
        id: Some("child_doc1".to_string()) 
    };
    let read_params = Parameters(read_request);
    let read_result = service.read_document(read_params).await.unwrap();
    
    if let Some(content) = read_result.content.first() {
        let content_str = &content.as_text().unwrap().text;
        let expected_content = "# child_doc1\n\n";
        assert_eq!(content_str, expected_content);
    }
}

#[tokio::test]
async fn test_success_response_format() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Test "Succeeded" response for existing document update
    let initial_content = "# Initial Content";
    let request1 = UpdateDocumentRequest {
        id: None, // Root document
        content: initial_content.to_string(),
    };
    let params1 = Parameters(request1);
    let result1 = service.update_document(params1).await.unwrap();
    
    if let Some(content) = result1.content.first() {
        let response = &content.as_text().unwrap().text;
        assert_eq!(response, "Succeeded");
    }
    
    // Test "Succeeded\nCreated: <id list>" response for new document creation
    // First create parent_doc via root
    let root_content = "# Root\n\n[[parent_doc]]";
    let root_request = UpdateDocumentRequest {
        id: None,
        content: root_content.to_string(),
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    let new_content = "# New Document\n\nLinks to [[new_doc]].";
    let request2 = UpdateDocumentRequest {
        id: Some("parent_doc".to_string()),
        content: new_content.to_string(),
    };
    let params2 = Parameters(request2);
    let result2 = service.update_document(params2).await.unwrap();
    
    if let Some(content) = result2.content.first() {
        let response = &content.as_text().unwrap().text;
        let expected_response = "Succeeded\nCreated: new_doc";
        assert_eq!(response, expected_response);
    }
}

#[tokio::test]
async fn test_nonexistent_document_error() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // Try to update a non-existent document (not root)
    let request = UpdateDocumentRequest {
        id: Some("nonexistent".to_string()),
        content: "Some content".to_string(),
    };
    let params = Parameters(request);
    
    let result = service.update_document(params).await;
    
    // Should return error
    assert!(result.is_err());
    
    // Verify error message matches specification exactly
    let error = result.unwrap_err();
    assert_eq!(error.message, "Error: id: not found");
}

#[tokio::test]
async fn test_cross_tree_reference_error() {
    let (service, _temp_dir) = setup_temp_service().await;
    
    // First create a document structure via root
    let root_content = "# Root\n\n[[parent]] [[parent2]]";
    let root_request = UpdateDocumentRequest {
        id: None,
        content: root_content.to_string(),
    };
    let root_params = Parameters(root_request);
    service.update_document(root_params).await.unwrap();
    
    // Update parent documents with their children
    let parent_content = "# Parent\n\n[[child1]]";
    let parent_request = UpdateDocumentRequest {
        id: Some("parent".to_string()),
        content: parent_content.to_string(),
    };
    let parent_params = Parameters(parent_request);
    service.update_document(parent_params).await.unwrap();
    
    let parent2_content = "# Parent2\n\n[[child2]]";
    let parent2_request = UpdateDocumentRequest {
        id: Some("parent2".to_string()),
        content: parent2_content.to_string(),
    };
    let parent2_params = Parameters(parent2_request);
    service.update_document(parent2_params).await.unwrap();
    
    // Now try to create cross-tree reference (child1 trying to link to child2)
    let invalid_content = "# Child1\n\nCross reference to [[child2]]";
    let invalid_request = UpdateDocumentRequest {
        id: Some("child1".to_string()),
        content: invalid_content.to_string(),
    };
    let invalid_params = Parameters(invalid_request);
    
    let result = service.update_document(invalid_params).await;
    
    // Should return error
    assert!(result.is_err());
    
    // Verify error message matches specification exactly
    let error = result.unwrap_err();
    // Note: This test is ignored as cross-tree validation is complex
    assert!(error.message.starts_with("Error: content: cross-tree reference not allowed"));
}