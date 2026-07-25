#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";

const REQUIRED_METHODS = [
  "tools.catalog",
  "tools.effective",
  "sessions.create",
  "sessions.resolve",
  "sessions.get",
  "sessions.subscribe",
  "sessions.messages.subscribe",
  "agent.wait",
  "chat.abort",
  "sessions.abort",
  "tasks.list",
  "tasks.get",
  "tasks.cancel",
  "models.list",
  "agents.list",
  "audit.list",
];

const APPROVED_TOOLS = [
  "catdesk-t0013a__catdesk_instruction",
  "catdesk-t0013a__plan_read",
  "catdesk-t0013a__read",
  "catdesk-t0013a__search",
];

const PROHIBITED_PATTERNS = [
  /(^|[_.:-])(bash|shell|exec|execute|command|terminal|powershell|cmd)([_.:-]|$)/i,
  /(^|[_.:-])(filesystem|file|fs|write|edit|delete|remove|move|copy)([_.:-]|$)/i,
  /apply[-_]?patch/i,
  /(^|[_.:-])(git|commit|push|merge|checkout|branch|reset)([_.:-]|$)/i,
  /(^|[_.:-])(browser|web|fetch|search|ui|computer|screen|automation|cron)([_.:-]|$)/i,
  /(^|[_.:-])(elevated|sudo|admin|code[-_]?mode)([_.:-]|$)/i,
];

function usage() {
  return `Usage:
  node scripts/openclaw_gateway_probe.mjs --out <path>
  node scripts/openclaw_gateway_probe.mjs --fixture <jsonl> --out <path>

Environment for live mode:
  OPENCLAW_GATEWAY_URL       ws://127.0.0.1:<port>
  OPENCLAW_GATEWAY_TOKEN     disposable shared token
  OPENCLAW_GATEWAY_AGENT_ID  OpenClaw agent id
  OPENCLAW_GATEWAY_SESSION_KEY  session key for this validation
`;
}

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--out") {
      args.out = argv[++i];
    } else if (arg === "--fixture") {
      args.fixture = argv[++i];
    } else if (arg === "--help" || arg === "-h") {
      args.help = true;
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

function redact(value) {
  if (Array.isArray(value)) {
    return value.map(redact);
  }
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, item] of Object.entries(value)) {
      if (/token|password|secret|credential|authorization|api[_-]?key|capability|pluginSurfaceUrls/i.test(key)) {
        out[key] = "[REDACTED]";
      } else {
        out[key] = redact(item);
      }
    }
    return out;
  }
  if (typeof value === "string" && value.includes("/__openclaw__/cap/")) {
    return "[REDACTED_CAPABILITY_URL]";
  }
  return value;
}

function summarizePayload(payload) {
  if (Array.isArray(payload)) {
    return { type: "array", length: payload.length };
  }
  if (!payload || typeof payload !== "object") {
    return { type: typeof payload };
  }
  const keys = Object.keys(payload).sort();
  const summary = { type: "object", keys };
  for (const key of [
    "ok",
    "key",
    "sessionKey",
    "status",
    "subscribed",
    "nextCursor",
    "protocol",
    "profile",
  ]) {
    if (Object.hasOwn(payload, key)) {
      summary[key] = payload[key];
    }
  }
  if (Array.isArray(payload.notices)) {
    summary.notices = payload.notices;
  }
  for (const key of ["tools", "entries", "groups", "agents", "models", "tasks", "events", "messages"]) {
    if (Array.isArray(payload[key])) {
      summary[`${key}Count`] = payload[key].length;
    }
  }
  return summary;
}

function collectToolNames(value, names = new Set()) {
  if (Array.isArray(value)) {
    for (const item of value) collectToolNames(item, names);
    return names;
  }
  if (!value || typeof value !== "object") {
    return names;
  }
  if (typeof value.name === "string") names.add(value.name);
  if (typeof value.toolName === "string") names.add(value.toolName);
  if (typeof value.id === "string" && /__/.test(value.id)) names.add(value.id);
  for (const item of Object.values(value)) collectToolNames(item, names);
  return names;
}

function evaluateTools(payload) {
  const names = [...collectToolNames(payload)].sort();
  const approvedMissing = APPROVED_TOOLS.filter((tool) => !names.includes(tool));
  const prohibitedVisible = names.filter(
    (name) =>
      !APPROVED_TOOLS.includes(name) &&
      PROHIBITED_PATTERNS.some((pattern) => pattern.test(name)),
  );
  const extraVisible = names.filter((name) => !APPROVED_TOOLS.includes(name));
  return { names, approvedMissing, prohibitedVisible, extraVisible };
}

function methodStatus(method, response) {
  if (!response) return "not_called";
  if (response.ok) return "ok";
  const message = String(response.error?.message ?? "");
  if (/not found|required|timeout|no-active-run|invalid .*params/i.test(message)) {
    return "documented_method_application_error";
  }
  return "error";
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

class GatewayProbeClient {
  constructor({ url, token, out }) {
    this.url = url;
    this.token = token;
    this.out = out;
    this.ws = null;
    this.nextId = 1;
    this.pending = new Map();
    this.events = [];
    this.hello = null;
  }

  async connect() {
    if (!globalThis.WebSocket) {
      throw new Error("Node global WebSocket is unavailable");
    }
    this.ws = new WebSocket(this.url);
    this.ws.addEventListener("message", async (event) => {
      const raw = typeof event.data === "string" ? event.data : await event.data.text();
      this.handleFrame(JSON.parse(raw));
    });
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("gateway socket open timeout")), 10000);
      this.ws.addEventListener("open", () => {
        clearTimeout(timer);
        resolve();
      }, { once: true });
      this.ws.addEventListener("error", () => {
        clearTimeout(timer);
        reject(new Error("gateway socket error before open"));
      }, { once: true });
    });
    const hello = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("gateway hello timeout")), 15000);
      this.pending.set("connect", {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
    });
    this.hello = hello;
    return hello;
  }

  handleFrame(frame) {
    if (frame.type === "event") {
      this.events.push(frame);
      if (frame.event === "connect.challenge") {
        const nonce = frame.payload?.nonce;
        if (typeof nonce !== "string" || nonce.trim() === "") {
          this.pending.get("connect")?.reject(new Error("connect challenge missing nonce"));
          return;
        }
        this.sendConnect(nonce);
      }
      return;
    }
    if (frame.type === "res") {
      const pending = this.pending.get(frame.id);
      if (!pending) return;
      this.pending.delete(frame.id);
      if (frame.ok) {
        pending.resolve(frame.payload);
      } else {
        const error = new Error(frame.error?.message ?? `gateway request failed: ${frame.id}`);
        error.frame = frame;
        pending.reject(error);
      }
    }
  }

  sendConnect(nonce) {
    const id = "connect";
    const params = {
      minProtocol: 4,
      maxProtocol: 4,
      client: {
        id: "gateway-client",
        displayName: "CatDesk T-0013A Gateway Probe",
        version: "t-0013a",
        platform: process.platform,
        mode: "backend",
        instanceId: randomUUID(),
      },
      role: "operator",
      scopes: ["operator.read", "operator.write"],
      caps: ["tool-events"],
      commands: [],
      permissions: {},
      auth: { token: this.token },
      locale: "en-US",
      userAgent: "catdesk-t0013a-gateway-probe/0.1",
      device: undefined,
    };
    this.ws.send(JSON.stringify({ type: "req", id, method: "connect", params }));
  }

  request(method, params = {}, timeoutMs = 10000) {
    const id = `req-${this.nextId++}`;
    const frame = { type: "req", id, method, params };
    const promise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`gateway request timeout: ${method}`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (payload) => {
          clearTimeout(timer);
          resolve({ ok: true, payload });
        },
        reject: (error) => {
          clearTimeout(timer);
          resolve({
            ok: false,
            error: redact(error.frame?.error ?? { message: error.message }),
          });
        },
      });
    });
    this.ws.send(JSON.stringify(frame));
    return promise;
  }

  async close() {
    if (!this.ws) return;
    const ws = this.ws;
    await new Promise((resolve) => {
      const timer = setTimeout(resolve, 1000);
      ws.addEventListener("close", () => {
        clearTimeout(timer);
        resolve();
      }, { once: true });
      ws.close();
    });
  }
}

async function runLive(outPath) {
  const url = process.env.OPENCLAW_GATEWAY_URL;
  const token = process.env.OPENCLAW_GATEWAY_TOKEN;
  const agentId = process.env.OPENCLAW_GATEWAY_AGENT_ID || "catdesk-t0013a-audit";
  const sessionKey = process.env.OPENCLAW_GATEWAY_SESSION_KEY || "agent:catdesk-t0013a-audit:t0013a";
  if (!url || !token) {
    throw new Error("OPENCLAW_GATEWAY_URL and OPENCLAW_GATEWAY_TOKEN are required");
  }
  const parsed = new URL(url);
  if (parsed.protocol !== "ws:" || !["127.0.0.1", "localhost", "::1"].includes(parsed.hostname)) {
    throw new Error(`Gateway URL must be loopback ws:// for this validation: ${url}`);
  }
  const client = new GatewayProbeClient({ url, token, out: outPath });
  const responses = {};
  try {
    const hello = await client.connect();
    const advertised = new Set(hello.features?.methods ?? []);
    const missingAdvertisedMethods = REQUIRED_METHODS.filter((method) => !advertised.has(method));

    responses["tools.catalog"] = await client.request("tools.catalog", { agentId, includePlugins: true });
    responses["sessions.create"] = await client.request("sessions.create", {
      key: sessionKey,
      agentId,
      label: "CatDesk T-0013A disposable validation",
    });
    responses["sessions.resolve"] = await client.request("sessions.resolve", {
      key: sessionKey,
      agentId,
      allowMissing: true,
    });
    responses["sessions.get"] = await client.request("sessions.get", { key: sessionKey, agentId, limit: 10 });
    responses["sessions.subscribe"] = await client.request("sessions.subscribe", {});
    responses["sessions.messages.subscribe"] = await client.request("sessions.messages.subscribe", { key: sessionKey, agentId });
    const effectiveAttempts = [];
    for (let attempt = 1; attempt <= 4; attempt += 1) {
      responses["tools.effective"] = await client.request("tools.effective", { sessionKey, agentId });
      const evaluated = evaluateTools(responses["tools.effective"]?.payload);
      effectiveAttempts.push({
        attempt,
        ok: responses["tools.effective"].ok,
        payloadSummary: responses["tools.effective"].ok
          ? summarizePayload(responses["tools.effective"].payload)
          : undefined,
        toolNames: evaluated.names,
        approvedMissing: evaluated.approvedMissing,
        prohibitedVisible: evaluated.prohibitedVisible,
      });
      if (evaluated.approvedMissing.length === 0 && evaluated.prohibitedVisible.length === 0) break;
      await sleep(3000);
    }
    responses["agent.wait"] = await client.request("agent.wait", { runId: "catdesk-t0013a-no-run", timeoutMs: 0 }, 2000);
    responses["sessions.abort"] = await client.request("sessions.abort", { key: sessionKey, agentId });
    responses["chat.abort"] = await client.request("chat.abort", { sessionKey, agentId });
    responses["tasks.list"] = await client.request("tasks.list", { limit: 10 });
    responses["tasks.get"] = await client.request("tasks.get", { taskId: "catdesk-t0013a-no-task" });
    responses["tasks.cancel"] = await client.request("tasks.cancel", {
      taskId: "catdesk-t0013a-no-task",
      reason: "disposable validation probe",
    });
    responses["models.list"] = await client.request("models.list", { view: "configured" });
    responses["agents.list"] = await client.request("agents.list", {});
    responses["audit.list"] = await client.request("audit.list", { limit: 10 });

    await new Promise((resolve) => setTimeout(resolve, 1000));

    const effective = evaluateTools(responses["tools.effective"]?.payload);
    const catalog = evaluateTools(responses["tools.catalog"]?.payload);
    const methodResults = Object.fromEntries(
      REQUIRED_METHODS.map((method) => [method, methodStatus(method, responses[method])]),
    );
    const operationalMissingMethods = REQUIRED_METHODS.filter((method) => methodResults[method] === "not_called");
    const eventSeqs = client.events
      .map((event) => event.seq)
      .filter((seq) => typeof seq === "number");
    const seqMonotonic = eventSeqs.every((seq, index) => index === 0 || seq > eventSeqs[index - 1]);
    const failClosedReasons = [];
    if (operationalMissingMethods.length > 0) {
      failClosedReasons.push(`required Gateway methods unavailable: ${operationalMissingMethods.join(", ")}`);
    }
    if (effective.approvedMissing.length > 0) {
      failClosedReasons.push(`tools.effective missing approved tools: ${effective.approvedMissing.join(", ")}`);
    }
    if (effective.prohibitedVisible.length > 0) {
      failClosedReasons.push(`tools.effective exposes prohibited tools: ${effective.prohibitedVisible.join(", ")}`);
    }
    if (eventSeqs.length === 0 || !seqMonotonic) {
      failClosedReasons.push("structured event sequencing was not observed reliably");
    }
    const output = {
      generatedAt: new Date().toISOString(),
      mode: "live",
      openclawVersionPinned: "2026.7.1-2",
      gateway: { url: `${parsed.protocol}//${parsed.hostname}:${parsed.port}`, auth: "token:[REDACTED]" },
      agentId,
      sessionKey,
      hello: redact(hello),
      missingAdvertisedMethods,
      operationalMissingMethods,
      methodResults,
      eventSummary: {
        count: client.events.length,
        names: [...new Set(client.events.map((event) => event.event))].sort(),
        seqs: eventSeqs,
        seqMonotonic,
      },
      toolPolicy: {
        approvedTools: APPROVED_TOOLS,
        catalog,
        effective,
      },
      responses: Object.fromEntries(
        Object.entries(responses).map(([method, response]) => [
          method,
          {
            ok: response.ok,
            status: methodStatus(method, response),
            payloadSummary: response.ok ? summarizePayload(response.payload) : undefined,
            error: response.ok ? undefined : response.error,
          },
        ]),
      ),
      effectiveAttempts,
      redactedToolPolicyPayloads: {
        catalog: redact(responses["tools.catalog"]?.payload),
        effective: redact(responses["tools.effective"]?.payload),
      },
      failClosed: failClosedReasons.length > 0,
      failClosedReasons,
    };
    await writeFile(outPath, `${JSON.stringify(output, null, 2)}\n`);
    if (output.failClosed) {
      throw new Error(`Gateway validation failed closed: ${failClosedReasons.join("; ")}`);
    }
  } finally {
    await client.close();
  }
}

async function runFixture(fixturePath, outPath) {
  const text = await readFile(fixturePath, "utf8");
  const frames = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const eventSeqs = frames
    .filter((frame) => frame.type === "event")
    .map((event) => event.seq)
    .filter((seq) => typeof seq === "number");
  const hello = frames.find((frame) => frame.type === "res" && frame.id === "connect" && frame.ok)?.payload;
  const methods = new Set(hello?.features?.methods ?? []);
  const missingAdvertisedMethods = REQUIRED_METHODS.filter((method) => !methods.has(method));
  const effectiveFrame = frames.find((frame) => frame.type === "res" && frame.id === "tools.effective" && frame.ok);
  const effective = evaluateTools(effectiveFrame?.payload);
  const output = {
    generatedAt: new Date().toISOString(),
    mode: "fixture",
    fixturePath,
    frameCount: frames.length,
    missingAdvertisedMethods,
    eventSummary: {
      count: frames.filter((frame) => frame.type === "event").length,
      seqs: eventSeqs,
      seqMonotonic: eventSeqs.every((seq, index) => index === 0 || seq > eventSeqs[index - 1]),
    },
    toolPolicy: {
      approvedTools: APPROVED_TOOLS,
      effective,
    },
    failClosed:
      missingAdvertisedMethods.length > 0 ||
      effective.approvedMissing.length > 0 ||
      effective.prohibitedVisible.length > 0 ||
      eventSeqs.length === 0,
  };
  await writeFile(outPath, `${JSON.stringify(output, null, 2)}\n`);
  if (output.failClosed) {
    throw new Error("fixture validation failed closed");
  }
}

const args = parseArgs(process.argv.slice(2));
if (args.help || !args.out) {
  console.log(usage());
  process.exit(args.help ? 0 : 2);
}

if (args.fixture) {
  await runFixture(args.fixture, args.out);
} else {
  await runLive(args.out);
}
