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
- Scheduling system with cron expressions
- Memory persistence and chat history

## Development Commands

### Main Application (Rust)
```bash
# Build the application
cargo build --release

# Run with different modes
cargo run -- --audio                    # Enable audio/voice features
cargo run -- --interactive              # Enable TUI interface
cargo run -- --scheduler                # Enable scheduling system
cargo run -- --notifier                 # Enable notifications
cargo run -- --audio --notifier --scheduler  # Combined modes

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
- **Scheduler** (`src/components/scheduler.rs`): Cron-based task scheduling
- **Notifier** (`src/components/notifier.rs`): Push notifications via FCM
- **Web** (`src/components/web.rs`): Web interface and API
- **Interactive** (`src/components/interactive.rs`): TUI interface with ratatui

### Key Adapters
- **OpenAI** (`src/adapter/openai.rs`): Chat completion and function calling
- **Dify** (`src/adapter/dify.rs`): Code execution in sandbox
- **FCM** (`src/adapter/fcm.rs`): Firebase Cloud Messaging

### Configuration
- Production config: `conf/default.toml`
- Development config: `conf/local.toml` (if exists)
- Environment variables: `PROMPT_PRIVATE_KEY`, `OPENAI_API_KEY`, `WEB_AUTH_TOKEN`, `DIFY_SANDBOX_API_KEY`

### Function Calling
The AI core supports function calling for:
- `memorize_function`: Store and retrieve memories
- `execute_code_function`: Run code in dify-sandbox
- `manage_schedule_function`: Manage cron schedules

### Data Persistence
- Chat history: `/var/memory/history.json`
- Memories: Stored in repository
- Schedules: Persisted in repository

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
- `src/common/repository.rs`: Data persistence layer
- `gui/src/routes/+page.svelte`: Main GUI interface
- `compose.yaml`: Docker service definitions
- `Taskfile.yaml`: Development and deployment tasks