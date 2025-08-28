use regex::Regex;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadDocumentRequest {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateDocumentRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TreeNode {
    children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StructuredMemoryService {
    tool_router: ToolRouter<Self>,
    data_dir: String,
    link_regex: Regex,
}

impl StructuredMemoryService {
    pub fn new(data_dir: String) -> Self {
        Self {
            tool_router: Self::tool_router(),
            data_dir,
            link_regex: Regex::new(r"\[\[([a-zA-Z0-9_-]+)\]\]").unwrap(),
        }
    }

    async fn ensure_data_dir(&self) -> Result<(), ErrorData> {
        if !Path::new(&self.data_dir).exists() {
            fs::create_dir_all(&self.data_dir).await.map_err(|e| {
                ErrorData::internal_error(
                    "Failed to create data directory",
                    Some(json!(
                        {"reason": e.to_string()}
                    )),
                )
            })?;
        }
        Ok(())
    }

    async fn document_path(&self, id: &str) -> String {
        format!("{}/{}.md", self.data_dir, id)
    }

    async fn document_exists(&self, id: &str) -> bool {
        let path = self.document_path(id).await;
        Path::new(&path).exists()
    }

    async fn ensure_root_document(&self) -> Result<(), ErrorData> {
        self.ensure_data_dir().await?;

        let root_path = self.document_path("root").await;
        if !Path::new(&root_path).exists() {
            let initial_content =
                "# Root Document\n\nThis is the root document of your structured memory.\n";
            fs::write(&root_path, initial_content).await.map_err(|e| {
                ErrorData::internal_error(
                    "Failed to create root document",
                    Some(json!(
                        {"reason": e.to_string()}
                    )),
                )
            })?;
        }
        Ok(())
    }

    fn extract_links(&self, content: &str) -> Vec<String> {
        self.link_regex
            .captures_iter(content)
            .map(|cap| cap[1].to_string())
            .collect()
    }

    async fn get_all_documents(&self) -> Result<HashSet<String>, ErrorData> {
        self.ensure_data_dir().await?;

        let mut documents = HashSet::new();
        let mut entries = fs::read_dir(&self.data_dir).await.map_err(|e| {
            ErrorData::internal_error(
                "Failed to read data directory",
                Some(json!(
                    {"reason": e.to_string()}
                )),
            )
        })?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with(".md") {
                    let doc_id = filename.strip_suffix(".md").unwrap().to_string();
                    documents.insert(doc_id);
                }
            }
        }

        Ok(documents)
    }

    async fn build_tree(&self) -> Result<HashMap<String, TreeNode>, ErrorData> {
        let documents = self.get_all_documents().await?;
        let mut tree = HashMap::new();

        for doc_id in &documents {
            let path = self.document_path(doc_id).await;
            if let Ok(content) = fs::read_to_string(&path).await {
                let links = self.extract_links(&content);
                tree.insert(doc_id.clone(), TreeNode { children: links });
            }
        }

        Ok(tree)
    }

    fn validate_tree_links(
        &self,
        parent_id: &str,
        links: &[String],
        tree: &HashMap<String, TreeNode>,
    ) -> Result<(), ErrorData> {
        for link in links {
            if let Some(existing_node) = tree.get(link) {
                for child_link in &existing_node.children {
                    if child_link == parent_id {
                        return Err(ErrorData::internal_error(
                            format!(
                                "Error: content: cross-tree reference not allowed - circular reference detected between {} and {}",
                                parent_id, link
                            ),
                            None,
                        ));
                    }
                }

                let existing_links = &existing_node.children;
                for existing_link in existing_links {
                    if links.contains(existing_link) && existing_link != link {
                        return Err(ErrorData::internal_error(
                            format!(
                                "Error: content: cross-tree reference not allowed - {} would create a complex cross-reference",
                                existing_link
                            ),
                            None,
                        ));
                    }
                }
            }

            // Check for cross-tree references: ensure link doesn't belong to another parent
            for (other_parent_id, other_node) in tree {
                if other_parent_id != parent_id && other_node.children.contains(link) {
                    return Err(ErrorData::internal_error(
                        format!(
                            "Error: content: cross-tree reference not allowed - {} already belongs to {}",
                            link, other_parent_id
                        ),
                        None,
                    ));
                }
            }
        }
        Ok(())
    }

    async fn cleanup_orphaned_documents(&self) -> Result<Vec<String>, ErrorData> {
        let tree = self.build_tree().await?;
        let mut referenced_docs = HashSet::new();
        referenced_docs.insert("root".to_string()); // Root is always kept

        // Collect all referenced documents
        for node in tree.values() {
            for child in &node.children {
                referenced_docs.insert(child.clone());
            }
        }

        let mut removed = Vec::new();
        for doc_id in tree.keys() {
            if !referenced_docs.contains(doc_id) && doc_id != "root" {
                let path = self.document_path(doc_id).await;
                if let Ok(()) = fs::remove_file(&path).await {
                    removed.push(doc_id.clone());
                }
            }
        }

        Ok(removed)
    }
}

#[tool_router]
impl StructuredMemoryService {
    #[tool(description = "Reads the content of a document")]
    pub async fn read_document(
        &self,
        params: Parameters<ReadDocumentRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let doc_id = request.id.as_deref().unwrap_or("root");

        self.ensure_root_document().await?;

        if !self.document_exists(doc_id).await {
            return Err(ErrorData::internal_error("Error: id: not found", None));
        }

        let path = self.document_path(doc_id).await;
        let content = fs::read_to_string(&path).await.map_err(|e| {
            ErrorData::internal_error(
                "Failed to read document",
                Some(json!(
                    {"reason": e.to_string()}
                )),
            )
        })?;

        Ok(CallToolResult {
            content: vec![Content::text(content)],
            structured_content: None,
            is_error: Some(false),
        })
    }

    #[tool(description = "Updates the content of a document")]
    pub async fn update_document(
        &self,
        params: Parameters<UpdateDocumentRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let doc_id = request.id.as_deref().unwrap_or("root");

        self.ensure_root_document().await?;

        if doc_id != "root" && !self.document_exists(doc_id).await {
            return Err(ErrorData::internal_error("Error: id: not found", None));
        }

        let links = self.extract_links(&request.content);
        let tree = self.build_tree().await?;

        // Validate tree structure before applying changes
        self.validate_tree_links(doc_id, &links, &tree)?;

        let path = self.document_path(doc_id).await;
        fs::write(&path, &request.content).await.map_err(|e| {
            ErrorData::internal_error(
                "Failed to write document",
                Some(json!(
                    {"reason": e.to_string()}
                )),
            )
        })?;

        let mut created_docs = Vec::new();
        for link in &links {
            if !self.document_exists(link).await {
                let new_path = self.document_path(link).await;
                let initial_content = format!("# {}\n\n", link);
                fs::write(&new_path, initial_content).await.map_err(|e| {
                    ErrorData::internal_error(
                        "Failed to create linked document",
                        Some(json!(
                            {"reason": e.to_string()}
                        )),
                    )
                })?;
                created_docs.push(link.clone());
            }
        }

        // Clean up orphaned documents
        self.cleanup_orphaned_documents().await?;

        let response = if created_docs.is_empty() {
            "Succeeded".to_string()
        } else {
            format!("Succeeded\nCreated: {}", created_docs.join(", "))
        };

        Ok(CallToolResult {
            content: vec![Content::text(response)],
            structured_content: None,
            is_error: Some(false),
        })
    }

    #[tool(description = "Returns the complete document tree structure")]
    pub async fn get_document_tree(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_root_document().await?;

        let tree = self.build_tree().await?;

        // Convert to YAML-like structure
        fn build_yaml_node(
            node_id: &str,
            tree: &HashMap<String, TreeNode>,
            visited: &mut HashSet<String>,
        ) -> serde_yaml::Value {
            if visited.contains(node_id) {
                return serde_yaml::Value::String(format!("{} (circular)", node_id));
            }
            visited.insert(node_id.to_string());

            if let Some(node) = tree.get(node_id) {
                if node.children.is_empty() {
                    serde_yaml::Value::String(node_id.to_string())
                } else {
                    let mut map = serde_yaml::Mapping::new();
                    let children: Vec<serde_yaml::Value> = node
                        .children
                        .iter()
                        .map(|child| build_yaml_node(child, tree, visited))
                        .collect();
                    map.insert(
                        serde_yaml::Value::String(node_id.to_string()),
                        serde_yaml::Value::Sequence(children),
                    );
                    serde_yaml::Value::Mapping(map)
                }
            } else {
                serde_yaml::Value::String(node_id.to_string())
            }
        }

        let mut visited = HashSet::new();
        let yaml_tree = build_yaml_node("root", &tree, &mut visited);

        let yaml_string = serde_yaml::to_string(&yaml_tree).map_err(|e| {
            ErrorData::internal_error(
                "Failed to serialize tree",
                Some(json!(
                    {"reason": e.to_string()}
                )),
            )
        })?;

        Ok(CallToolResult {
            content: vec![Content::text(yaml_string)],
            structured_content: None,
            is_error: Some(false),
        })
    }
}

#[tool_handler]
impl ServerHandler for StructuredMemoryService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Structured memory MCP server for hierarchical document management with [[link]] syntax".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation { name: env!("CARGO_CRATE_NAME").to_owned(), version: env!("CARGO_PKG_VERSION").to_owned() },
            ..Default::default()
        }
    }
}
