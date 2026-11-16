const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');

const PORT = Number(process.env.PORT || 3000);
const SCORE_FILE = process.env.SCORE_FILE || path.join(__dirname, 'data', 'scores.json');
const FRONTEND_ORIGIN = 'http://localhost:5173';

function sortScores(scores) {
  return scores
    .map((entry, index) => ({ ...entry, index }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, 10)
    .map(({ name, score }) => ({ name, score }));
}

class ScoreStore {
  constructor(file) {
    this.file = file;
    this.scores = null;
    this.queue = Promise.resolve();
  }

  async load() {
    if (this.scores) return this.scores;
    try {
      const parsed = JSON.parse(await fs.readFile(this.file, 'utf8'));
      if (!Array.isArray(parsed) || parsed.some((entry) =>
        !entry || typeof entry.name !== 'string' || !Number.isSafeInteger(entry.score) || entry.score < 0
      )) throw new Error('Score storage contains invalid data');
      this.scores = sortScores(parsed);
    } catch (error) {
      if (error.code === 'ENOENT') this.scores = [];
      else throw error;
    }
    return this.scores;
  }

  async list() {
    return (await this.load()).map((entry) => ({ ...entry }));
  }

  add(entry) {
    const operation = this.queue.then(async () => {
      const current = await this.load();
      const next = sortScores([...current, entry]);
      await fs.mkdir(path.dirname(this.file), { recursive: true });
      const temporary = `${this.file}.${process.pid}.${Date.now()}.tmp`;
      try {
        await fs.writeFile(temporary, `${JSON.stringify(next, null, 2)}\n`, { flag: 'wx' });
        await fs.rename(temporary, this.file);
      } catch (error) {
        await fs.rm(temporary, { force: true }).catch(() => {});
        throw error;
      }
      this.scores = next;
    });
    this.queue = operation.catch(() => {});
    return operation;
  }
}

function validateSubmission(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) throw new Error('Request body must be a JSON object');
  if (typeof body.name !== 'string') throw new Error('Name must be a string');
  const name = body.name.trim();
  if (name.length < 1 || name.length > 20) throw new Error('Name must be 1 to 20 characters');
  if (!Number.isSafeInteger(body.score) || body.score < 0) throw new Error('Score must be a nonnegative safe integer');
  return { name, score: body.score };
}

function sendJson(response, status, payload) {
  response.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' });
  response.end(JSON.stringify(payload));
}

function createServer(store = new ScoreStore(SCORE_FILE)) {
  return http.createServer(async (request, response) => {
    if (request.headers.origin === FRONTEND_ORIGIN) {
      response.setHeader('Access-Control-Allow-Origin', FRONTEND_ORIGIN);
      response.setHeader('Vary', 'Origin');
      response.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
      response.setHeader('Access-Control-Allow-Headers', 'Content-Type');
    }
    if (request.method === 'OPTIONS') {
      response.writeHead(204);
      response.end();
      return;
    }
    const url = new URL(request.url, 'http://localhost');
    if (url.pathname !== '/api/scores') {
      sendJson(response, 404, { error: 'Not found' });
      return;
    }
    if (request.method === 'GET') {
      try {
        sendJson(response, 200, { scores: await store.list() });
      } catch (error) {
        sendJson(response, 500, { error: error.message || 'Storage failure' });
      }
      return;
    }
    if (request.method !== 'POST') {
      response.setHeader('Allow', 'GET, POST, OPTIONS');
      sendJson(response, 405, { error: 'Method not allowed' });
      return;
    }
    let raw = '';
    try {
      for await (const chunk of request) {
        raw += chunk;
        if (raw.length > 1024 * 1024) throw new Error('Request body is too large');
      }
      let body;
      try { body = JSON.parse(raw); } catch { throw new Error('Malformed JSON'); }
      const entry = validateSubmission(body);
      await store.add(entry);
      sendJson(response, 201, entry);
    } catch (error) {
      const clientError = error instanceof SyntaxError || error.message === 'Malformed JSON' ||
        error.message.startsWith('Request body') || error.message.startsWith('Name ') ||
        error.message.startsWith('Score ') || error.message.startsWith('Request body must');
      sendJson(response, clientError ? 400 : 500, { error: error.message || 'Storage failure' });
    }
  });
}

if (require.main === module) {
  createServer().listen(PORT, () => console.log(`Score API listening on http://localhost:${PORT}`));
}

module.exports = { ScoreStore, createServer, sortScores, validateSubmission };
