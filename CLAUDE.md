# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tsuki is a kawaii chat agent built with Rust that provides:
- Event-driven architecture with an actor-like component system
- Multi-modal interaction (text, audio, notifications)
- AI-powered chat using OpenAI API with function calling
- Code execution capabilities via dify-sandbox
- Cross-platform GUI client (desktop and Android) built with Tauri + Svelte
- Voice recognition and synthesis via Vosk and VoiceVox
- Memory persistence and chat history

## Development Commands

### Main Application (Rust)
```bash
# Build the application
cargo build --release

# Run with different modes
cargo run -- --audio                    # Enable audio/voice features
cargo run -- --interactive              # Enable TUI interface
cargo run -- --notifier                 # Enable notifications
cargo run -- --audio --notifier         # Combined modes

# Run tests
cargo test

# Development with local config
cargo run   # Uses conf/local.toml in debug mode
```

### GUI Client
```bash
cd gui/
npm run dev          # Development server
npm run build        # Build for production
npm run check        # Type checking
npm run tauri dev    # Run Tauri development
npm run tauri build  # Build Tauri app
```

### Docker/Production
```bash
# Using Taskfile (task runner)
task deploy          # Deploy all services
task deploy-app      # Deploy only the app
task up              # Start services
task down            # Stop services
task build           # Build all images
task build-app       # Build specific service
task log-app         # View logs for app service

# Direct docker compose
docker compose up --build --detach

# Check running services
docker ps            # List running containers
docker compose ps    # List compose services

# Execute commands in containers
docker compose exec app cargo build                    # Build in container
docker compose exec app cargo test                     # Run tests in container
docker compose exec -w /workspace app cargo run        # Run application in container
docker compose exec app bash                           # Interactive shell in app container

# Service-specific operations
docker compose logs app                                 # View app logs
docker compose restart app                             # Restart app service
docker compose exec qdrant curl http://localhost:6333/collections  # Check Qdrant collections
```

### Voice Model Setup
```bash
task download_model  # Download Japanese Vosk model
```

### Prompt Management (Encrypted)
```bash
task decrypt_prompt  # Decrypt prompt file for editing
task encrypt_prompt  # Encrypt prompt file after editing
task diff_prompt     # Compare encrypted vs current
```

## Architecture

### Layered Architecture
- **Components** (`src/components/`): High-level application components
- **Adapter** (`src/adapter/`): External service integrations
- **Common** (`src/common/`): Shared utilities and data structures

### Event System
The application uses an event-driven architecture (`src/common/events.rs`):
- `EventSystem`: Central event bus that coordinates all components
- `EventComponent`: Trait that all components implement
- Components communicate through events: `TextMessage`, `AssistantMessage`, `RecognizedSpeech`, `PlayAudio`, `Notify`

### Core Components
- **Core** (`src/components/core/`): Main AI chat engine with OpenAI integration
- **Recognizer** (`src/components/recognizer.rs`): Speech recognition via Vosk
- **Speak** (`src/components/speak.rs`): Text-to-speech via VoiceVox
- **Notifier** (`src/components/notifier.rs`): Push notifications via FCM
- **Web** (`src/components/web.rs`): Web interface and API
- **Interactive** (`src/components/interactive.rs`): TUI interface with ratatui

### Key Adapters
- **OpenAI** (`src/adapter/openai.rs`): Chat completion and function calling
- **EmbeddingService** (`src/adapter/embedding.rs`): OpenAI text embeddings for vector search
- **Dify** (`src/adapter/dify.rs`): Code execution in sandbox
- **FCM** (`src/adapter/fcm.rs`): Firebase Cloud Messaging

### Configuration
- Production config: `conf/default.toml`
- Development config: `conf/local.toml` (if exists)
- Environment variables: `PROMPT_PRIVATE_KEY`, `OPENAI_API_KEY`, `WEB_AUTH_TOKEN`, `DIFY_SANDBOX_API_KEY`

### Docker Services
The application runs with multiple services via Docker Compose:
- **app**: Main Rust application (tsuki)
- **qdrant**: Vector database for semantic search (port 6333-6334)
- **ssrf-proxy**: Secure proxy for dify-sandbox (port 3128, 8194)
- **sandbox**: Code execution environment (dify-sandbox)
- **mumble-server**: Voice chat server (port 64738)
- **voicevox-engine**: Text-to-speech engine (port 50021)

### Function Calling
The AI core supports function calling for:
- `memorize_function`: Store and retrieve memories with vector search
- `execute_code_function`: Run code in dify-sandbox

### Data Persistence
- Chat history and messages: Stored in repository (file or Qdrant)
- Memories: Stored in repository with vector embeddings for semantic search
- Vector database: Qdrant with OpenAI embeddings (optional)

### Repository System
- **FileRepository**: JSON-based persistence for development
- **QdrantRepository**: Vector database with semantic search capabilities
- **RepositoryFactory**: Clean dependency injection with automatic EmbeddingService setup
- **Design**: Schedule functionality omitted in favor of MCP plugin implementation

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

## Key Files to Understand

- `src/main.rs`: Application entry point and component orchestration
- `src/common/events.rs`: Event system implementation
- `src/components/core/mod.rs`: AI chat engine
- `src/repository/mod.rs`: Repository trait and RepositoryFactory
- `src/repository/qdrant.rs`: Vector database implementation with semantic search
- `src/repository/file.rs`: JSON-based file repository for development
- `src/adapter/embedding.rs`: OpenAI embedding service for vector search
- `gui/src/routes/+page.svelte`: Main GUI interface
- `compose.yaml`: Docker service definitions
- `Taskfile.yaml`: Development and deployment tasks
- `doc/qdrant_repository_implementation.md`: Detailed implementation documentation

## Schedule Functionality

**Note**: Schedule functionality has been intentionally omitted from the core system. 

**Rationale**: 
- LLM difficulties with time calculations and cron expressions
- Better separation of concerns
- Improved maintainability

**Recommended Approach**: 
- Implement scheduling as MCP (Model Context Protocol) plugin
- External scheduling systems can integrate via MCP interface
- Allows specialized time management without core complexity