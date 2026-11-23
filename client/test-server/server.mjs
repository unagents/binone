#!/usr/bin/env node
// A stand-in for the binone server, for checking the client by hand. TEST ONLY: it is not part
// of the app and is not the real server.
//
// It answers the way the client expects the server to:
//   POST /key      -> 200 {"key":"<string>"}            (403 with --mode forbidden)
//   POST /graphql  -> 200 {"data":{"serverInfo":{"version":"..."}}} when the request carries
//                     `Authorization: Bearer <key>`; 401 otherwise
//
// Usage: node test-server/server.mjs [--port <n>] [--host <addr>] [--mode <mode>]
//   --port  default 47610 (the client's default port)
//   --host  default 127.0.0.1
//   --mode  ok           follow the contract (default)
//           forbidden    POST /key answers 403 (the client is outside the allowed IP range)
//           rotate       every start issues a new key, so a key saved earlier gets 401 once
//           reject       every key gets 401, even a new one
//           bad-key      POST /key answers 200 with an unexpected body
//           bad-info     POST /graphql answers 200 with an unexpected body
// In the modes other than `rotate` the key is fixed, so a client that connected before a
// restart of this server stays connected.
import { randomBytes } from "node:crypto";
import { createServer } from "node:http";

const args = process.argv.slice(2);
function option(name, fallback) {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
}
const port = Number(option("port", "47610"));
const host = option("host", "127.0.0.1");
const mode = option("mode", "ok");
const MODES = ["ok", "forbidden", "rotate", "reject", "bad-key", "bad-info"];
if (!MODES.includes(mode) || !Number.isInteger(port)) {
  console.error(`usage: server.mjs [--port <n>] [--host <addr>] [--mode ${MODES.join("|")}]`);
  process.exit(2);
}

const VERSION = "0.0.0-test";
const key = mode === "rotate" ? `test-${randomBytes(8).toString("hex")}` : "test-key";

function send(response, status, body) {
  const text = body === undefined ? "" : JSON.stringify(body);
  response.writeHead(status, { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(text) });
  response.end(text);
}

const server = createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    const authorized = mode !== "reject" && request.headers.authorization === `Bearer ${key}`;
    let status;
    if (request.method === "POST" && request.url === "/key") {
      if (mode === "forbidden") status = send(response, 403) ?? 403;
      else if (mode === "bad-key") status = send(response, 200, { token: key }) ?? 200;
      else status = send(response, 200, { key }) ?? 200;
    } else if (request.method === "POST" && request.url === "/graphql") {
      if (!authorized) status = send(response, 401) ?? 401;
      else if (mode === "bad-info") status = send(response, 200, { data: { server: {} } }) ?? 200;
      else status = send(response, 200, { data: { serverInfo: { version: VERSION } } }) ?? 200;
    } else {
      status = send(response, 404) ?? 404;
    }
    // The log shows whether a key came with the request, never the key itself.
    const auth = request.headers.authorization === undefined ? "no key" : authorized ? "valid key" : "invalid key";
    console.log(`${request.method} ${request.url} (${auth}) -> ${status}`);
  });
});
server.listen(port, host, () => console.log(`test server (${mode}) on http://${host}:${port}`));
