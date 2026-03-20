# Infra — Overview

Infrastructure covers container orchestration, CI/CD, and operational tooling. The runtime is
deployed as a set of Docker Compose services managed through Taskfile.

## Services

| Service | Role |
|---|---|
| `core` | Main backend runtime (core-rust) |
| `memgraph` | Concept graph storage (vector + graph queries) |
| `sandbox` | gVisor-isolated MCP shell execution environment |
| `ja-accent` | Japanese accent annotation service |
| `voicevox` | TTS synthesis engine |

## Key Constraints

- Production deployment goes through CI/CD only (`github/workflows/`). Local `task`/`docker
  compose` is not a production deployment path.
- A change is not done until its delivery path is updated: env vars introduced in code must be
  propagated to `compose.yaml`, CI/CD secret mapping, and operator docs in the same change.
- `DOCKER_HOST` for production is defined in `.env`; never hardcode it.
- Stop the `core` container before restoring a Memgraph snapshot.

## Configuration Split

- Secrets: environment variables only (`WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `ADMIN_AUTH_PASSWORD`,
  `MEMGRAPH_PASSWORD`, `TURSO_AUTH_TOKEN`).
- Non-secrets: `config.toml` with no env fallback.
