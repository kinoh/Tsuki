# Structured memory for LLM agents

## Features

- Manages Markdown documents on the filesystem
  - Initially, only one root document exists
  - Writing links in `[[document_id]]` format creates a document with ID `document_id`
  - Documents are deleted when their links are removed
- Simple interface (context-efficient) designed for persistent MCP server connections
- Tree structure integrity maintained through parent-child link constraints

## Usage Patterns

### Tree-based Knowledge Organization

This system is designed for building hierarchical knowledge structures that grow naturally:

1. **Start with a root document** containing high-level topics
2. **Extract details into subdocuments** as content grows using `[[subdocument_id]]` links
3. **Build knowledge trees** through natural linking progression
4. **Automatic pruning** removes unused branches when links are deleted

### Example Workflow

```markdown
# Root Document
- Project Overview [[project_overview]]
- Technical Decisions [[tech_decisions]]
- Current Tasks [[current_tasks]]

# project_overview document expands to:
- Architecture [[architecture]]
- Dependencies [[dependencies]]
- API Design [[api_design]]

# architecture document further expands to:
- Database Schema [[db_schema]]
- Service Layer [[service_layer]]
```

### Tree Growth Pattern

- Agent intentionally splits documents when they become too large or complex
- Each level focuses on appropriate detail granularity
- Links represent logical parent-child relationships
- Tree structure remains navigable and maintainable

## Tools

### read_document

Reads the content of a document.

#### Arguments

- id: (Optional) ID of the target document. If omitted, reads the root document.

#### Response

Document content in Markdown format.

#### Errors

- Returns `Error: id: not found` if the specified document ID does not exist.

### update_document

Updates the content of a document.

#### Arguments

- id: (Optional) ID of the target document. If omitted, targets the root document.
- content: (Required) The new content.

#### Response

Returns `Succeeded\nCreated: <id list>` if documents were created.
Returns `Succeeded` if no documents were created.

#### Errors

- Returns `Error: id: not found` if the specified document ID does not exist.
- Returns `Error: content: cross-tree reference not allowed` if content contains links to documents outside the direct child hierarchy.

### get_document_tree

Returns the complete document tree structure showing all documents and their relationships.

#### Arguments

None.

#### Response

YAML structure representing the tree hierarchy:

```yaml
root:
  - project_overview:
    - architecture:
      - db_schema
      - service_layer
    - dependencies
  - tech_decisions:
    - decision_001
    - decision_002
```

#### Errors

None. Always returns the current tree structure.

## Tree Integrity Constraints

To maintain tree structure integrity:

- **Parent-Child Links Only**: Documents may only link to their direct children
- **No Cross-Tree References**: Links to sibling documents or distant relatives are not allowed
- **Automatic Validation**: The `update_document` tool validates all links before applying changes
- **Error Prevention**: Invalid link structures are rejected with clear error messages

This ensures the document collection remains a proper tree without cycles or complex cross-references.
