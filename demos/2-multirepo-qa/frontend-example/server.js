const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');

const PORT = Number(process.env.PORT || 5173);
const ROOT = __dirname;
const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8', '.js': 'text/javascript; charset=utf-8' };

http.createServer(async (request, response) => {
  const pathname = new URL(request.url, 'http://localhost').pathname;
  const requested = pathname === '/' ? '/index.html' : decodeURIComponent(pathname);
  const file = path.resolve(ROOT, `.${requested}`);
  if (!file.startsWith(`${ROOT}${path.sep}`)) {
    response.writeHead(403).end('Forbidden');
    return;
  }
  try {
    const content = await fs.readFile(file);
    response.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] || 'application/octet-stream' });
    response.end(content);
  } catch {
    response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' }).end('Not found');
  }
}).listen(PORT, () => console.log(`Snake game listening on http://localhost:${PORT}`));
