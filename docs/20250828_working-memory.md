# Working Memory Implementation

## Overview

The working memory feature (feat/working-memory) was implemented to address scenarios where semantic recall cannot adequately handle information retention. This system provides hierarchical document management with automatic link resolution and tree structure integrity.

## Problem Statement

### Background
- **Semantic Recall Limitations**: Standard semantic recall couldn't handle all conversation patterns effectively
- **Notion MCP Issues**: Initial consideration of Notion remote MCP server faced several challenges:
  - Interface was markdown-formatted, solving Notion API complexity
  - Long descriptions caused rapid token consumption
  - Remote API dependency increased technical footprint unnecessarily

### Solution Requirements
- Local file-based storage to minimize token consumption
- Hierarchical organization to handle information growth
- Tree structure maintenance with automatic document management
- Simple interface optimized for persistent MCP server connections

## Architecture

### Structured Memory MCP Server
- **Language**: Rust with MCP SDK (rmcp crate)
- **Protocol**: Model Context Protocol (MCP) for tool integration
- **Storage**: File-based persistence with markdown documents
- **Location**: `mcp/structured-memory/`

### Core Components

#### MCP Server (`structured-memory`)
- **Binary**: `./bin/structured-memory`
- **Data Directory**: `${DATA_DIR}/structured_memory`
- **Root Template**: `# メモ帳\n` (Japanese "notebook")

#### Integration Points
- **MCP Configuration**: `core/src/mastra/mcp.ts`
- **Prompt Integration**: Enhanced agent prompts with structured-memory usage instructions
- **Testing**: `core/scripts/test_memory.js` for memory capability validation

## Features

### Document Management
- **Root Document**: Single entry point initialized with ROOT_TEMPLATE
- **Link Syntax**: `[[document_id]]` creates and links to subdocuments
- **Auto Creation**: Referenced documents are automatically created with basic template
- **Auto Cleanup**: Orphaned documents are automatically removed when no longer referenced

### Tree Structure Integrity
- **Parent-Child Links Only**: Documents may only link to direct children
- **Cross-Tree Prevention**: Links to sibling or distant documents are rejected
- **Circular Reference Detection**: Prevents creation of document cycles
- **Validation**: All link structures validated before applying changes

### MCP Tools

#### `read_document`
- **Purpose**: Reads content of specified document
- **Parameters**: `id` (optional, defaults to "root")
- **Response**: Document content in markdown format
- **Errors**: `Error: id: not found` for non-existent documents

#### `update_document`
- **Purpose**: Updates document content and processes links
- **Parameters**: `id` (optional, defaults to "root"), `content` (required)
- **Response**: 
  - `Succeeded` if no new documents created
  - `Succeeded\nCreated: <list>` if documents were created
- **Validation**: Prevents cross-tree references and circular links
- **Side Effects**: Creates linked documents, cleans up orphans

#### `get_document_tree`
- **Purpose**: Returns complete hierarchical structure
- **Parameters**: None
- **Response**: YAML-formatted tree structure starting from root
- **Format**: Nested structure showing parent-child relationships

## Implementation Details

### File Structure
```
mcp/structured-memory/
├── Cargo.toml              # Rust project configuration
├── src/
│   ├── main.rs            # MCP server entry point
│   ├── service.rs         # Core implementation
│   └── lib.rs             # Library exports
├── tests/                 # Comprehensive test suite
└── README.md             # Usage documentation
```

### Link Processing
- **Regex Pattern**: `\[\[([a-zA-Z0-9_-]+)\]\]`
- **Document Creation**: Auto-generates `# {document_id}\n\n` template
- **Link Validation**: Enforces tree structure constraints before saving

### Tree Integrity Constraints
1. **Parent-Child Only**: Documents can only reference their direct children
2. **No Cross-References**: Prevents links between siblings or distant relatives  
3. **Circular Prevention**: Detects and rejects circular reference attempts
4. **Merge Prevention**: Prevents multiple parents from linking to same document

## Usage Patterns

### Natural Growth Model
1. Start with root document containing high-level topics
2. Extract details into subdocuments using `[[subdocument_id]]` links
3. Build knowledge trees through natural linking progression
4. System automatically maintains tree integrity and prunes unused branches

### Example Workflow
```markdown
# Root Document (メモ帳)
- Project Overview [[project_overview]]
- Technical Decisions [[tech_decisions]]
- Current Tasks [[current_tasks]]

# project_overview expands to:
- Architecture [[architecture]]
- Dependencies [[dependencies]]
- API Design [[api_design]]

# architecture further expands to:
- Database Schema [[db_schema]]
- Service Layer [[service_layer]]
```

## Development Approach

### Migration from Notion MCP
- **Commit `4e74e3f`**: Replaced Notion MCP with lightweight structured-memory
- **Rationale**: Reduced token consumption and removed external API dependency
- **Benefits**: Faster response times, lower operational costs, simpler deployment

### Test-Driven Development
- **Unit Tests**: Comprehensive Rust test suite validates all tool behaviors
- **Integration Tests**: Memory capability tests verify end-to-end functionality
- **Test Environment**: Isolated temporary directories for safe testing

### Configuration Management
- **Environment Variables**: `DATA_DIR`, `ROOT_TEMPLATE` for flexible deployment
- **MCP Integration**: Seamless tool discovery and invocation
- **Prompt Enhancement**: Agent instructions updated to utilize structured memory

## Future Extensions

### Potential Features
- **Management UI**: Simple web interface for document navigation
- **External Sync**: Integration with Notion, Obsidian, or other knowledge bases
- **Resource Integration**: Unified file system representation of diverse resources
- **Memory Reorganization**: Periodic "sleep" mode for automatic document organization

### Scalability Considerations
- **Document Splitting**: Automatic subdivision of oversized documents
- **Depth Limits**: Configurable maximum tree depth
- **Performance**: Efficient tree traversal for large document collections
- **Search**: Full-text search across document collection

## Testing and Validation

### Memory Test Patterns
- **Book List Scenario**: Tests information retention across conversation phases
- **Reduced Semantic Recall**: Configured with minimal settings to stress-test working memory
- **Multi-Agent Testing**: Different memory configurations for comparative analysis

### Test Commands
```bash
# Run structured-memory unit tests
cd mcp/structured-memory && cargo test

# Run memory capability integration test  
cd core && node scripts/test_memory.js

# Build and install structured-memory binary
cd mcp/structured-memory && cargo build --release
cp target/release/structured-memory ../../core/bin/
```

## Deployment

### Environment Setup
```bash
export DATA_DIR="/path/to/data"
export ROOT_TEMPLATE="# メモ帳\n"
```

### Binary Installation
```bash
cd mcp/structured-memory
cargo build --release
cp target/release/structured-memory ../core/bin/
```

### MCP Server Integration
The structured-memory server is automatically configured in the MCP client and available to agents through the standard tool invocation mechanism.

## Technical Decisions

### Why Rust?
- **Performance**: Fast document processing and tree validation
- **Safety**: Memory safety prevents data corruption
- **MCP SDK**: Native rmcp crate support for protocol implementation
- **Concurrency**: Tokio async runtime for efficient I/O operations

### Why Local Files?
- **Token Efficiency**: Eliminates verbose API descriptions
- **Simplicity**: Direct file system operations without network complexity  
- **Reliability**: No external service dependencies
- **Privacy**: Sensitive information remains on local system

### Why Tree Structure?
- **Cognitive Model**: Matches natural knowledge organization patterns
- **Scalability**: Hierarchical organization handles growth effectively
- **Integrity**: Enforced structure prevents information fragmentation
- **Navigation**: Clear parent-child relationships aid in exploration

## Impact

The structured-memory implementation successfully addresses the original problem of semantic recall limitations while providing a foundation for future knowledge management enhancements. The lightweight, file-based approach offers significant advantages in token efficiency and system reliability compared to remote API alternatives.