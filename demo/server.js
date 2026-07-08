// Demo file: JavaScript syntax highlighting showcase.

const http = require("http");

const PORT = 8080;
const routes = new Map();

function route(path, handler) {
  routes.set(path, handler);
}

route("/", async (req, res) => {
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ hello: "plume", weight: 0.0062 }));
});

route("/health", (req, res) => {
  res.end(`ok ${Date.now()}`);
});

/* A tiny router — no framework needed
   for a demo this small. */
const server = http.createServer((req, res) => {
  const handler = routes.get(req.url);
  if (!handler) {
    res.writeHead(404);
    return res.end("not found");
  }
  handler(req, res);
});

server.listen(PORT, () => console.log(`listening on :${PORT}`));
