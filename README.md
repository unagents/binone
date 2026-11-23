# binone

A task-driven supporter that works on top of bantou and openspec-mcp.
The user registers a request; binone carries it forward without further instructions.

## Layout

- `client/`: the client app (Tauri 2, Nuxt 4, TypeScript, Tailwind CSS, Nuxt UI). So far it has
  one screen: the server's IP is entered, the client gets a key from the server, saves both and
  shows the server's version. The HTTP calls and the stored key stay on the native (Rust) side.
  - `test-server/server.mjs`: a stand-in for the server, for checking the client by hand (test only)
  - `Casks/binone.rb`: source of the Homebrew cask
  - `scripts/release.sh`: build, register the dmg, update the tap
- `docs/release.md`: how to install the client with Homebrew and how to release it
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

## Client development

```sh
cd client
bun install
bun run tauri dev      # run the app
bun run typecheck && bun run lint && bun run test
(cd src-tauri && cargo test)
bun run tauri build    # binone.app under src-tauri/target/release/bundle/macos/ (the dmg is made by scripts/release.sh)
```

Node.js 20.19 or later, bun, Rust and the Xcode Command Line Tools are required.

To try the client without the real server, run the stand-in and connect to `127.0.0.1`:

```sh
node client/test-server/server.mjs            # follows the contract
node client/test-server/server.mjs --mode forbidden   # see the file for the other modes
```
