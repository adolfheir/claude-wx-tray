#!/usr/bin/env bun
/**
 * Claude Code WeChat Channel Plugin
 *
 * Bridges WeChat messages into a Claude Code session via the Channels MCP protocol.
 * Uses the official WeChat ClawBot ilink API (same as @tencent-weixin/openclaw-weixin).
 *
 * Flow:
 *   1. QR login via ilink/bot/get_bot_qrcode + get_qrcode_status
 *   2. Long-poll ilink/bot/getupdates for incoming WeChat messages
 *   3. Forward messages to Claude Code as <channel> events
 *   4. Expose a reply tool so Claude can send messages back via ilink/bot/sendmessage
 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  ListToolsRequestSchema,
  CallToolRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

// ── Config ────────────────────────────────────────────────────────────────────

const CHANNEL_NAME = "wechat";
const CHANNEL_VERSION = "0.1.0";
const DEFAULT_BASE_URL = "https://ilinkai.weixin.qq.com";
const CDN_BASE_URL = "https://novac2c.cdn.weixin.qq.com/c2c";
const BOT_TYPE = "3";
const CREDENTIALS_DIR = path.join(
  process.env.HOME || "~",
  ".claude",
  "channels",
  "wechat",
);
const CREDENTIALS_FILE = path.join(CREDENTIALS_DIR, "account.json");

const LONG_POLL_TIMEOUT_MS = 35_000;
const MAX_CONSECUTIVE_FAILURES = 3;
const BACKOFF_DELAY_MS = 30_000;
const RETRY_DELAY_MS = 2_000;

// ── Logging (stderr only — stdout is MCP stdio) ─────────────────────────────

function log(msg: string) {
  process.stderr.write(`[wechat-channel] ${msg}\n`);
}

function logError(msg: string) {
  process.stderr.write(`[wechat-channel] ERROR: ${msg}\n`);
}

// ── Credentials ──────────────────────────────────────────────────────────────

type AccountData = {
  token: string;
  baseUrl: string;
  accountId: string;
  userId?: string;
  savedAt: string;
};

function loadCredentials(): AccountData | null {
  try {
    if (!fs.existsSync(CREDENTIALS_FILE)) return null;
    return JSON.parse(fs.readFileSync(CREDENTIALS_FILE, "utf-8"));
  } catch {
    return null;
  }
}

function saveCredentials(data: AccountData): void {
  fs.mkdirSync(CREDENTIALS_DIR, { recursive: true });
  fs.writeFileSync(CREDENTIALS_FILE, JSON.stringify(data, null, 2), "utf-8");
  try {
    fs.chmodSync(CREDENTIALS_FILE, 0o600);
  } catch {
    // best-effort
  }
}

// ── WeChat ilink API ─────────────────────────────────────────────────────────

function randomWechatUin(): string {
  const uint32 = crypto.randomBytes(4).readUInt32BE(0);
  return Buffer.from(String(uint32), "utf-8").toString("base64");
}

function buildHeaders(token?: string, body?: string): Record<string, string> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    AuthorizationType: "ilink_bot_token",
    "X-WECHAT-UIN": randomWechatUin(),
  };
  if (body) {
    headers["Content-Length"] = String(Buffer.byteLength(body, "utf-8"));
  }
  if (token?.trim()) {
    headers.Authorization = `Bearer ${token.trim()}`;
  }
  return headers;
}

async function apiFetch(params: {
  baseUrl: string;
  endpoint: string;
  body: string;
  token?: string;
  timeoutMs: number;
}): Promise<string> {
  const base = params.baseUrl.endsWith("/")
    ? params.baseUrl
    : `${params.baseUrl}/`;
  const url = new URL(params.endpoint, base).toString();
  const headers = buildHeaders(params.token, params.body);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), params.timeoutMs);
  try {
    const res = await fetch(url, {
      method: "POST",
      headers,
      body: params.body,
      signal: controller.signal,
    });
    clearTimeout(timer);
    const text = await res.text();
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${text}`);
    return text;
  } catch (err) {
    clearTimeout(timer);
    throw err;
  }
}

// ── QR Login ─────────────────────────────────────────────────────────────────

interface QRCodeResponse {
  qrcode: string;
  qrcode_img_content: string;
}

interface QRStatusResponse {
  status: "wait" | "scaned" | "confirmed" | "expired";
  bot_token?: string;
  ilink_bot_id?: string;
  baseurl?: string;
  ilink_user_id?: string;
}

async function fetchQRCode(baseUrl: string): Promise<QRCodeResponse> {
  const base = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
  const url = new URL(
    `ilink/bot/get_bot_qrcode?bot_type=${encodeURIComponent(BOT_TYPE)}`,
    base,
  );
  const res = await fetch(url.toString());
  if (!res.ok) throw new Error(`QR fetch failed: ${res.status}`);
  return (await res.json()) as QRCodeResponse;
}

async function pollQRStatus(
  baseUrl: string,
  qrcode: string,
): Promise<QRStatusResponse> {
  const base = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
  const url = new URL(
    `ilink/bot/get_qrcode_status?qrcode=${encodeURIComponent(qrcode)}`,
    base,
  );
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 35_000);
  try {
    const res = await fetch(url.toString(), {
      headers: { "iLink-App-ClientVersion": "1" },
      signal: controller.signal,
    });
    clearTimeout(timer);
    if (!res.ok) throw new Error(`QR status failed: ${res.status}`);
    return (await res.json()) as QRStatusResponse;
  } catch (err) {
    clearTimeout(timer);
    if (err instanceof Error && err.name === "AbortError") {
      return { status: "wait" };
    }
    throw err;
  }
}

async function doQRLogin(
  baseUrl: string,
): Promise<AccountData | null> {
  log("正在获取微信登录二维码...");
  const qrResp = await fetchQRCode(baseUrl);

  log("\n请使用微信扫描以下二维码：\n");
  try {
    const qrterm = await import("qrcode-terminal");
    await new Promise<void>((resolve) => {
      qrterm.default.generate(
        qrResp.qrcode_img_content,
        { small: true },
        (qr: string) => {
          process.stderr.write(qr + "\n");
          resolve();
        },
      );
    });
  } catch {
    log(`二维码链接: ${qrResp.qrcode_img_content}`);
  }

  log("等待扫码...");
  const deadline = Date.now() + 480_000;
  let scannedPrinted = false;

  while (Date.now() < deadline) {
    const status = await pollQRStatus(baseUrl, qrResp.qrcode);

    switch (status.status) {
      case "wait":
        break;
      case "scaned":
        if (!scannedPrinted) {
          log("已扫码，请在微信中确认...");
          scannedPrinted = true;
        }
        break;
      case "expired":
        log("二维码已过期，请重新启动。");
        return null;
      case "confirmed": {
        if (!status.ilink_bot_id || !status.bot_token) {
          logError("登录确认但未返回 bot 信息");
          return null;
        }
        const account: AccountData = {
          token: status.bot_token,
          baseUrl: status.baseurl || baseUrl,
          accountId: status.ilink_bot_id,
          userId: status.ilink_user_id,
          savedAt: new Date().toISOString(),
        };
        saveCredentials(account);
        log("微信连接成功！");
        return account;
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }

  log("登录超时");
  return null;
}

// ── WeChat Message Types ─────────────────────────────────────────────────────

interface TextItem {
  text?: string;
}

interface RefMessage {
  message_item?: MessageItem;
  title?: string;
}

interface MessageItem {
  type?: number;
  text_item?: TextItem;
  voice_item?: { text?: string };
  ref_msg?: RefMessage;
}

interface WeixinMessage {
  from_user_id?: string;
  to_user_id?: string;
  client_id?: string;
  session_id?: string;
  message_type?: number;
  message_state?: number;
  item_list?: MessageItem[];
  context_token?: string;
  create_time_ms?: number;
}

interface GetUpdatesResp {
  ret?: number;
  errcode?: number;
  errmsg?: string;
  msgs?: WeixinMessage[];
  get_updates_buf?: string;
  longpolling_timeout_ms?: number;
}

// Message type constants
const MSG_TYPE_USER = 1;
const MSG_ITEM_TEXT = 1;
const MSG_ITEM_IMAGE = 2;
const MSG_ITEM_VOICE = 3;
const MSG_ITEM_FILE = 4;
const MSG_ITEM_VIDEO = 5;
const MSG_TYPE_BOT = 2;
const MSG_STATE_STREAMING = 1;
const MSG_STATE_FINISH = 2;

function extractTextFromMessage(msg: WeixinMessage): string {
  if (!msg.item_list?.length) return "";
  for (const item of msg.item_list) {
    if (item.type === MSG_ITEM_TEXT && item.text_item?.text) {
      const text = item.text_item.text;
      const ref = item.ref_msg;
      if (!ref) return text;
      const parts: string[] = [];
      if (ref.title) parts.push(ref.title);
      if (!parts.length) return text;
      return `[引用: ${parts.join(" | ")}]\n${text}`;
    }
    if (item.type === MSG_ITEM_IMAGE) return "[图片]";
    if (item.type === MSG_ITEM_VOICE && item.voice_item?.text) {
      return item.voice_item.text;
    }
    if (item.type === MSG_ITEM_FILE) return "[文件]";
    if (item.type === MSG_ITEM_VIDEO) return "[视频]";
  }
  return "";
}

// ── Context Token Cache ──────────────────────────────────────────────────────

const contextTokenCache = new Map<string, string>();

function cacheContextToken(userId: string, token: string): void {
  contextTokenCache.set(userId, token);
}

function getCachedContextToken(userId: string): string | undefined {
  return contextTokenCache.get(userId);
}

// ── getUpdates / sendMessage ─────────────────────────────────────────────────

async function getUpdates(
  baseUrl: string,
  token: string,
  getUpdatesBuf: string,
): Promise<GetUpdatesResp> {
  try {
    const raw = await apiFetch({
      baseUrl,
      endpoint: "ilink/bot/getupdates",
      body: JSON.stringify({
        get_updates_buf: getUpdatesBuf,
        base_info: { channel_version: CHANNEL_VERSION },
      }),
      token,
      timeoutMs: LONG_POLL_TIMEOUT_MS,
    });
    return JSON.parse(raw) as GetUpdatesResp;
  } catch (err) {
    if (err instanceof Error && err.name === "AbortError") {
      return { ret: 0, msgs: [], get_updates_buf: getUpdatesBuf };
    }
    throw err;
  }
}

function generateClientId(): string {
  return `claude-code-wechat:${Date.now()}-${crypto.randomBytes(4).toString("hex")}`;
}

async function sendTextMessage(
  baseUrl: string,
  token: string,
  to: string,
  text: string,
  contextToken: string,
): Promise<string> {
  const clientId = generateClientId();
  await apiFetch({
    baseUrl,
    endpoint: "ilink/bot/sendmessage",
    body: JSON.stringify({
      msg: {
        from_user_id: "",
        to_user_id: to,
        client_id: clientId,
        message_type: MSG_TYPE_BOT,
        message_state: MSG_STATE_FINISH,
        item_list: [{ type: MSG_ITEM_TEXT, text_item: { text } }],
        context_token: contextToken,
      },
      base_info: { channel_version: CHANNEL_VERSION },
    }),
    token,
    timeoutMs: 15_000,
  });
  return clientId;
}

// ── Image Upload & Send ──────────────────────────────────────────────────────

function aesEcbPaddedSize(plaintextSize: number): number {
  return Math.ceil((plaintextSize + 1) / 16) * 16;
}

function encryptAesEcb(plaintext: Buffer, key: Buffer): Buffer {
  const cipher = crypto.createCipheriv("aes-128-ecb", key, null);
  return Buffer.concat([cipher.update(plaintext), cipher.final()]);
}

interface UploadUrlResponse {
  upload_param?: string;
  [key: string]: unknown;
}

async function getUploadUrl(
  baseUrl: string,
  token: string,
  params: {
    filekey: string;
    mediaType: number;
    toUserId: string;
    rawsize: number;
    rawfilemd5: string;
    filesize: number;
    aeskey: string;
  },
): Promise<UploadUrlResponse> {
  const raw = await apiFetch({
    baseUrl,
    endpoint: "ilink/bot/getuploadurl",
    body: JSON.stringify({
      filekey: params.filekey,
      media_type: params.mediaType,
      to_user_id: params.toUserId,
      rawsize: params.rawsize,
      rawfilemd5: params.rawfilemd5,
      filesize: params.filesize,
      no_need_thumb: true,
      aeskey: params.aeskey,
    }),
    token,
    timeoutMs: 15_000,
  });
  return JSON.parse(raw) as UploadUrlResponse;
}

async function uploadBufferToCdn(params: {
  buf: Buffer;
  uploadParam: string;
  filekey: string;
  aeskey: Buffer;
}): Promise<string> {
  const encrypted = encryptAesEcb(params.buf, params.aeskey);
  const uploadUrl = `${CDN_BASE_URL}/upload?encrypted_query_param=${encodeURIComponent(params.uploadParam)}&filekey=${encodeURIComponent(params.filekey)}`;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 60_000);
  try {
    const res = await fetch(uploadUrl, {
      method: "POST",
      headers: {
        "Content-Type": "application/octet-stream",
        "Content-Length": String(encrypted.length),
      },
      body: new Uint8Array(encrypted),
      signal: controller.signal,
    });
    clearTimeout(timer);

    if (!res.ok) {
      const errMsg = res.headers.get("x-error-message") || (await res.text());
      throw new Error(`CDN upload failed: ${res.status} ${errMsg}`);
    }

    const downloadParam = res.headers.get("x-encrypted-param");
    if (!downloadParam) {
      throw new Error("CDN response missing x-encrypted-param header");
    }
    return downloadParam;
  } catch (err) {
    clearTimeout(timer);
    throw err;
  }
}

async function sendImageMessage(
  baseUrl: string,
  token: string,
  to: string,
  imagePath: string,
  contextToken: string,
): Promise<string> {
  const imageBuffer = fs.readFileSync(imagePath);
  const rawsize = imageBuffer.length;
  const rawfilemd5 = crypto.createHash("md5").update(imageBuffer).digest("hex");
  const filesize = aesEcbPaddedSize(rawsize);

  const filekey = crypto.randomBytes(16).toString("hex");
  const aeskey = crypto.randomBytes(16);

  log(`上传图片: ${imagePath} (${rawsize} bytes)`);

  const uploadResp = await getUploadUrl(baseUrl, token, {
    filekey,
    mediaType: 1, // IMAGE
    toUserId: to,
    rawsize,
    rawfilemd5,
    filesize,
    aeskey: aeskey.toString("hex"),
  });

  if (!uploadResp.upload_param) {
    throw new Error("getuploadurl response missing upload_param");
  }

  const downloadParam = await uploadBufferToCdn({
    buf: imageBuffer,
    uploadParam: uploadResp.upload_param,
    filekey,
    aeskey,
  });

  const aesKeyBase64 = Buffer.from(aeskey.toString("hex")).toString("base64");
  const clientId = generateClientId();

  await apiFetch({
    baseUrl,
    endpoint: "ilink/bot/sendmessage",
    body: JSON.stringify({
      msg: {
        from_user_id: "",
        to_user_id: to,
        client_id: clientId,
        message_type: MSG_TYPE_BOT,
        message_state: MSG_STATE_FINISH,
        context_token: contextToken,
        item_list: [
          {
            type: MSG_ITEM_IMAGE,
            image_item: {
              media: {
                encrypt_query_param: downloadParam,
                aes_key: aesKeyBase64,
                encrypt_type: 1,
              },
              mid_size: filesize,
            },
          },
        ],
      },
      base_info: { channel_version: CHANNEL_VERSION },
    }),
    token,
    timeoutMs: 15_000,
  });

  log(`图片发送成功: ${imagePath}`);
  return clientId;
}

// ── MCP Channel Server ──────────────────────────────────────────────────────

const mcp = new Server(
  { name: CHANNEL_NAME, version: CHANNEL_VERSION },
  {
    capabilities: {
      experimental: { "claude/channel": {} },
      tools: {},
    },
    instructions: [
      `Messages from WeChat users arrive as <channel source="wechat" sender="..." sender_id="...">`,
      "Reply using the wechat_reply tool for text, or wechat_send_image for images. You MUST pass the sender_id from the inbound tag.",
      "To send an image, first save it to a local file, then use wechat_send_image with the absolute file path.",
      "Messages are from real WeChat users via the WeChat ClawBot interface.",
      "Respond naturally in Chinese unless the user writes in another language.",
      "Keep replies concise — WeChat is a chat app, not an essay platform.",
      "Strip markdown formatting (WeChat doesn't render it). Use plain text.",
      "IMPORTANT: For tasks that take more than a few seconds, send intermediate progress updates to the user via wechat_reply.",
      "For example: '正在分析代码...' → (do work) → '已完成分析，正在生成修改...' → (do work) → final reply.",
      "This keeps the WeChat user informed that you are working on their request.",
    ].join("\n"),
  },
);

// Tools: reply to WeChat
mcp.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: "wechat_reply",
      description: "Send a text reply back to the WeChat user",
      inputSchema: {
        type: "object" as const,
        properties: {
          sender_id: {
            type: "string",
            description:
              "The sender_id from the inbound <channel> tag (xxx@im.wechat format)",
          },
          text: {
            type: "string",
            description: "The plain-text message to send (no markdown)",
          },
        },
        required: ["sender_id", "text"],
      },
    },
    {
      name: "wechat_send_image",
      description:
        "Send an image to the WeChat user. The image must be a local file path (jpg/png/gif/webp/bmp).",
      inputSchema: {
        type: "object" as const,
        properties: {
          sender_id: {
            type: "string",
            description:
              "The sender_id from the inbound <channel> tag (xxx@im.wechat format)",
          },
          image_path: {
            type: "string",
            description:
              "Absolute path to the image file on disk",
          },
        },
        required: ["sender_id", "image_path"],
      },
    },
  ],
}));

let activeAccount: AccountData | null = null;

mcp.setRequestHandler(CallToolRequestSchema, async (req) => {
  const toolName = req.params.name;

  if (toolName === "wechat_reply" || toolName === "wechat_send_image") {
    if (!activeAccount) {
      return {
        content: [{ type: "text" as const, text: "error: not logged in" }],
      };
    }

    const args = req.params.arguments as Record<string, string>;
    const senderId = args.sender_id;
    const contextToken = getCachedContextToken(senderId);
    if (!contextToken) {
      return {
        content: [
          {
            type: "text" as const,
            text: `error: no context_token for ${senderId}. The user may need to send a message first.`,
          },
        ],
      };
    }

    try {
      if (toolName === "wechat_reply") {
        await sendTextMessage(
          activeAccount.baseUrl,
          activeAccount.token,
          senderId,
          args.text,
          contextToken,
        );
      } else {
        const imagePath = args.image_path;
        if (!fs.existsSync(imagePath)) {
          return {
            content: [
              { type: "text" as const, text: `error: file not found: ${imagePath}` },
            ],
          };
        }
        await sendImageMessage(
          activeAccount.baseUrl,
          activeAccount.token,
          senderId,
          imagePath,
          contextToken,
        );
      }
      return { content: [{ type: "text" as const, text: "sent" }] };
    } catch (err) {
      return {
        content: [
          { type: "text" as const, text: `send failed: ${String(err)}` },
        ],
      };
    }
  }

  throw new Error(`unknown tool: ${toolName}`);
});

// ── Long-poll loop ──────────────────────────────────────────────────────────

async function startPolling(account: AccountData): Promise<never> {
  const { baseUrl, token } = account;
  let getUpdatesBuf = "";
  let consecutiveFailures = 0;

  // Load cached sync buf if available
  const syncBufFile = path.join(CREDENTIALS_DIR, "sync_buf.txt");
  try {
    if (fs.existsSync(syncBufFile)) {
      getUpdatesBuf = fs.readFileSync(syncBufFile, "utf-8");
      log(`恢复上次同步状态 (${getUpdatesBuf.length} bytes)`);
    }
  } catch {
    // ignore
  }

  log("开始监听微信消息...");

  while (true) {
    try {
      const resp = await getUpdates(baseUrl, token, getUpdatesBuf);

      // Handle API errors
      const isError =
        (resp.ret !== undefined && resp.ret !== 0) ||
        (resp.errcode !== undefined && resp.errcode !== 0);
      if (isError) {
        consecutiveFailures++;
        logError(
          `getUpdates 失败: ret=${resp.ret} errcode=${resp.errcode} errmsg=${resp.errmsg ?? ""}`,
        );
        if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
          logError(
            `连续失败 ${MAX_CONSECUTIVE_FAILURES} 次，等待 ${BACKOFF_DELAY_MS / 1000}s`,
          );
          consecutiveFailures = 0;
          await new Promise((r) => setTimeout(r, BACKOFF_DELAY_MS));
        } else {
          await new Promise((r) => setTimeout(r, RETRY_DELAY_MS));
        }
        continue;
      }

      consecutiveFailures = 0;

      // Save sync buf
      if (resp.get_updates_buf) {
        getUpdatesBuf = resp.get_updates_buf;
        try {
          fs.writeFileSync(syncBufFile, getUpdatesBuf, "utf-8");
        } catch {
          // ignore
        }
      }

      // Process messages
      for (const msg of resp.msgs ?? []) {
        // Only process user messages (not bot messages)
        if (msg.message_type !== MSG_TYPE_USER) continue;

        const text = extractTextFromMessage(msg);
        if (!text) continue;

        const senderId = msg.from_user_id ?? "unknown";

        // Cache context token for reply
        if (msg.context_token) {
          cacheContextToken(senderId, msg.context_token);
        }

        log(`收到消息: from=${senderId} text=${text.slice(0, 50)}...`);

        // Push to Claude Code session
        await mcp.notification({
          method: "notifications/claude/channel",
          params: {
            content: text,
            meta: {
              sender: senderId.split("@")[0] || senderId,
              sender_id: senderId,
            },
          },
        });
      }
    } catch (err) {
      consecutiveFailures++;
      logError(`轮询异常: ${String(err)}`);
      if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
        consecutiveFailures = 0;
        await new Promise((r) => setTimeout(r, BACKOFF_DELAY_MS));
      } else {
        await new Promise((r) => setTimeout(r, RETRY_DELAY_MS));
      }
    }
  }
}

// ── Main ────────────────────────────────────────────────────────────────────

async function main() {
  // Connect MCP transport first (Claude Code expects stdio handshake)
  await mcp.connect(new StdioServerTransport());
  log("MCP 连接就绪");

  // Check for saved credentials
  let account = loadCredentials();

  if (!account) {
    log("未找到已保存的凭据，启动微信扫码登录...");
    account = await doQRLogin(DEFAULT_BASE_URL);
    if (!account) {
      logError("登录失败，退出。");
      process.exit(1);
    }
  } else {
    log(`使用已保存账号: ${account.accountId}`);
  }

  activeAccount = account;

  // Start long-poll (runs forever)
  await startPolling(account);
}

main().catch((err) => {
  logError(`Fatal: ${String(err)}`);
  process.exit(1);
});
