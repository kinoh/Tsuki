#!/usr/bin/env node

import { WebSocket } from 'ws';
import * as readline from 'readline';

const WS_URL = process.env.WS_URL || 'ws://localhost:2953/';
const AUTH_TOKEN = process.env.WEB_AUTH_TOKEN || 'test-token';
const USER_NAME = process.env.USER_NAME || 'test-user';

console.log(`Connecting to: ${WS_URL}`);
console.log(`Auth: ${USER_NAME}:${AUTH_TOKEN}`);

const ws = new WebSocket(WS_URL);

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  prompt: '> '
});

ws.on('open', () => {
  console.log('✅ Connected to WebSocket server');

  ws.send(`${USER_NAME}:${AUTH_TOKEN}`);

  console.log('📝 Type messages and press Enter (Ctrl+C to exit)');
  rl.prompt();
});

ws.on('message', (data) => {
  try {
    const message = JSON.parse(data.toString());
    console.log('\n📨 Received:');
    console.log(JSON.stringify(message, null, 2));
  } catch (error) {
    console.log('\n📨 Raw message:', data.toString());
  }
  rl.prompt();
});

ws.on('close', () => {
  console.log('\n❌ Connection closed');
  process.exit(0);
});

ws.on('error', (error) => {
  console.error('\n💥 WebSocket error:', error.message);
  process.exit(1);
});

rl.on('line', (input) => {
  const message = input.trim();
  if (message) {
    // Message with prefix "sensory:" treated as sensory message
    const isSensory = message.startsWith('sensory:');
    const payload = {
      type: (isSensory ? 'sensory' : 'message'),
      text: message.replace(/^sensory:/, '').trim(),
    };
    console.log('📤 Sending:', JSON.stringify(payload));
    ws.send(JSON.stringify(payload));
  }
  rl.prompt();
});

rl.on('SIGINT', () => {
  console.log('\n👋 Bye!');
  ws.close();
  process.exit(0);
});
