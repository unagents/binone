# binone

A task-driven supporter that works on top of bantou and openspec-mcp.
The user registers a request; binone carries it forward without further instructions.

## Layout

- `prototype/`: static HTML prototypes of the client screens (appearance only; dummy data).
  - `swimlane.html`: swimlane (kanban) of requests by lane
  - `request-register.html`: request registration with the drafting-assistant chat
  - `request-list.html`: request list and request status
  - `notification.html`: notifications

## Documents

Domain knowledge and design documents (requirements, design, architecture, server spec)
are kept outside this repository, in the project's knowledge vault.
Environment-specific information (hosts, addresses) is not stored here.

## Planned stack

- Client: Tauri, Nuxt 4, TypeScript, Tailwind CSS, Nuxt UI
- Server: Rust (GraphQL endpoint for the client screens, MCP tools for agents)
