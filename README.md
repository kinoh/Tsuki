## Tsuki: Kawaii chat agent

### Features

- **TypeScript/Mastra Backend**: Modern AI agent framework with built-in memory
- **WebSocket & HTTP APIs**: Real-time communication and RESTful endpoints
- **Smart Thread Management**: Automatic conversation continuation with daily thread IDs
- **Cross-thread Semantic Recall**: Persistent memory across conversation sessions
- **Encrypted Prompts**: Secure agent instruction storage using Age encryption
- **MCP Tool Integration**: Extensible tool system via Model Context Protocol
- **GUI Client**: Cross-platform desktop and Android app built with [Tauri](https://tauri.app/) + Svelte
- **Code Execution**: Sandboxed code execution via [dify-sandbox](https://github.com/langgenius/dify-sandbox)

### Quick Start

```bash
# Start the backend
cd core/
npm install
npm start

# Start the GUI client (separate terminal)
cd gui/
npm run tauri dev

# Or deploy with Docker
task deploy  # or docker compose up --build --detach
```

### Architecture

- **Core Backend** (`core/`): TypeScript/Node.js server with Mastra agents
- **GUI Client** (`gui/`): Tauri + Svelte cross-platform application
- **Docker Services**: Microservices for sandboxing, voice, and other integrations

See [CLAUDE.md](CLAUDE.md) for detailed development documentation.

### License

This software includes modified versions of the following components:

#### [Dify](https://github.com/langgenius/dify)

The following files incorporate code from Dify:

- /docker/**
- /compose.yaml


```
# Open Source License

Dify is licensed under the Apache License 2.0, with the following additional conditions:

1. Dify may be utilized commercially, including as a backend service for other applications or as an application development platform for enterprises. Should the conditions below be met, a commercial license must be obtained from the producer:

a. Multi-tenant service: Unless explicitly authorized by Dify in writing, you may not use the Dify source code to operate a multi-tenant environment. 
    - Tenant Definition: Within the context of Dify, one tenant corresponds to one workspace. The workspace provides a separated area for each tenant's data and configurations.
    
b. LOGO and copyright information: In the process of using Dify's frontend, you may not remove or modify the LOGO or copyright information in the Dify console or applications. This restriction is inapplicable to uses of Dify that do not involve its frontend.
    - Frontend Definition: For the purposes of this license, the "frontend" of Dify includes all components located in the `web/` directory when running Dify from the raw source code, or the "web" image when running Dify with Docker.

Please contact business@dify.ai by email to inquire about licensing matters.

2. As a contributor, you should agree that:

a. The producer can adjust the open-source agreement to be more strict or relaxed as deemed necessary.
b. Your contributed code may be used for commercial purposes, including but not limited to its cloud business operations.

Apart from the specific conditions mentioned above, all other rights and restrictions follow the Apache License 2.0. Detailed information about the Apache License 2.0 can be found at http://www.apache.org/licenses/LICENSE-2.0.

The interactive design of this product is protected by appearance patent.

© 2024 LangGenius, Inc.


----------

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
