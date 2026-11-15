'use strict';

const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');

const HOST = '0.0.0.0';
const PORT = 3000;
const DATA_FILE = path.join(__dirname, 'scores.json');
const MAX_BODY_BYTES = 16 * 1024;
const API_PATH = '/api/high-scores';

let pendingWrite = Promise.resolve();

function response(res, status, value) {
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
  });
  res.end(JSON.stringify(value));
}

async function readScores() {
  let content;
  try {
    content = await fs.readFile(DATA_FILE, 'utf8');
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const initial = Buffer.from('[]\n');
    try {
      await fs.writeFile(DATA_FILE, initial, { flag: 'wx' });
      return [];
    } catch (createError) {
      if (createError.code !== 'EEXIST') throw createError;
      content = await fs.readFile(DATA_FILE, 'utf8');
    }
  }

  let scores;
  try {
    scores = JSON.parse(content);
  } catch {
    throw new Error('Score storage contains malformed JSON');
  }
  if (!Array.isArray(scores) || !scores.every((entry) =>
    entry && typeof entry.name === 'string' && entry.name.length >= 1 && entry.name.length <= 20 &&
    Number.isSafeInteger(entry.score) && entry.score >= 0)) {
    throw new Error('Score storage has an invalid format');
  }
  return scores;
}

function saveScores(scores) {
  const temporary = `${DATA_FILE}.${process.pid}.${Date.now()}.tmp`;
  return fs.writeFile(temporary, `${JSON.stringify(scores, null, 2)}\n`, { flag: 'wx' })
    .then(() => fs.rename(temporary, DATA_FILE))
    .catch(async (error) => {
      await fs.rm(temporary, { force: true }).catch(() => {});
      throw error;
    });
}

function requestBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    let oversized = false;
    req.on('data', (chunk) => {
      if (oversized) return;
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        oversized = true;
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => {
      if (oversized) reject(Object.assign(new Error('Request body is too large'), { status: 413 }));
      else resolve(Buffer.concat(chunks).toString('utf8'));
    });
    req.on('error', reject);
  });
}

const server = http.createServer(async (req, res) => {
  if (req.method === 'OPTIONS') {
    response(res, 204, {});
    return;
  }
  if (req.url !== API_PATH) {
    response(res, 404, { error: 'Not found' });
    return;
  }
  if (req.method === 'GET') {
    try {
      response(res, 200, { scores: await readScores() });
    } catch (error) {
      console.error('Unable to read score storage:', error.message);
      response(res, 500, { error: 'Unable to read high scores' });
    }
    return;
  }
  if (req.method !== 'POST') {
    response(res, 405, { error: 'Method not allowed' });
    return;
  }
  if (!/^application\/json(?:\s*;|\s*$)/i.test(req.headers['content-type'] || '')) {
    response(res, 400, { error: 'Content-Type must be application/json' });
    return;
  }

  let submission;
  try {
    const body = await requestBody(req);
    submission = JSON.parse(body);
  } catch (error) {
    if (res.destroyed) return;
    response(res, error.status || 400, { error: error.status ? error.message : 'Request body must be valid JSON' });
    return;
  }
  if (!submission || typeof submission !== 'object' || Array.isArray(submission)) {
    response(res, 400, { error: 'Request body must be a JSON object' });
    return;
  }
  if (typeof submission.name !== 'string') {
    response(res, 400, { error: 'Name must be a string' });
    return;
  }
  const name = submission.name.trim();
  if (name.length < 1 || name.length > 20) {
    response(res, 400, { error: 'Name must be 1 to 20 characters after trimming' });
    return;
  }
  if (!Number.isSafeInteger(submission.score) || submission.score < 0) {
    response(res, 400, { error: 'Score must be a nonnegative safe integer' });
    return;
  }

  const operation = pendingWrite.then(async () => {
    const scores = await readScores();
    scores.push({ name, score: submission.score });
    scores.sort((a, b) => b.score - a.score);
    const topScores = scores.slice(0, 10);
    await saveScores(topScores);
    return topScores;
  });
  pendingWrite = operation.catch(() => {});
  try {
    response(res, 201, { scores: await operation });
  } catch (error) {
    console.error('Unable to persist high score:', error.message);
    response(res, 500, { error: 'Unable to save high score' });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`High-score API listening on http://${HOST}:${PORT}`);
});

function shutdown() {
  server.close(() => process.exit(0));
}
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
