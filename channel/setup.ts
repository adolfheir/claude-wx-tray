#!/usr/bin/env bun
/**
 * WeChat Channel Setup — standalone QR login tool.
 * Credentials are saved to ~/.claude/channels/wechat/account.json.
 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_BASE_URL = "https://ilinkai.weixin.qq.com";
const BOT_TYPE = "3";
const CREDENTIALS_DIR = path.join(
  process.env.HOME || "~",
  ".claude",
  "channels",
  "wechat",
);
const CREDENTIALS_FILE = path.join(CREDENTIALS_DIR, "account.json");

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
  const url = `${base}ilink/bot/get_bot_qrcode?bot_type=${BOT_TYPE}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`QR fetch failed: ${res.status}`);
  return (await res.json()) as QRCodeResponse;
}

async function pollQRStatus(
  baseUrl: string,
  qrcode: string,
): Promise<QRStatusResponse> {
  const base = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
  const url = `${base}ilink/bot/get_qrcode_status?qrcode=${encodeURIComponent(qrcode)}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 35_000);
  try {
    const res = await fetch(url, {
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

async function main() {
  console.log("正在获取微信登录二维码...\n");
  const qrResp = await fetchQRCode(DEFAULT_BASE_URL);

  try {
    const qrterm = await import("qrcode-terminal");
    await new Promise<void>((resolve) => {
      qrterm.default.generate(
        qrResp.qrcode_img_content,
        { small: true },
        (qr: string) => {
          console.log(qr);
          resolve();
        },
      );
    });
  } catch {
    console.log(`请在浏览器中打开此链接扫码: ${qrResp.qrcode_img_content}\n`);
  }

  console.log("请用微信扫描上方二维码...\n");

  const deadline = Date.now() + 480_000;
  let scannedPrinted = false;

  while (Date.now() < deadline) {
    const status = await pollQRStatus(DEFAULT_BASE_URL, qrResp.qrcode);

    switch (status.status) {
      case "wait":
        process.stdout.write(".");
        break;
      case "scaned":
        if (!scannedPrinted) {
          console.log("\n已扫码，请在微信中确认...");
          scannedPrinted = true;
        }
        break;
      case "expired":
        console.log("\n二维码已过期，请重启应用重试。");
        process.exit(1);
        break;
      case "confirmed": {
        if (!status.ilink_bot_id || !status.bot_token) {
          console.error("\n登录失败：服务器未返回完整信息。");
          process.exit(1);
        }

        const account = {
          token: status.bot_token,
          baseUrl: status.baseurl || DEFAULT_BASE_URL,
          accountId: status.ilink_bot_id,
          userId: status.ilink_user_id,
          savedAt: new Date().toISOString(),
        };

        fs.mkdirSync(CREDENTIALS_DIR, { recursive: true });
        fs.writeFileSync(
          CREDENTIALS_FILE,
          JSON.stringify(account, null, 2),
          "utf-8",
        );
        try {
          fs.chmodSync(CREDENTIALS_FILE, 0o600);
        } catch {
          // best-effort
        }

        console.log(`\n微信连接成功！`);
        console.log(`   账号 ID: ${account.accountId}`);
        console.log(`   凭据保存至: ${CREDENTIALS_FILE}`);
        process.exit(0);
      }
    }
    await new Promise((r) => setTimeout(r, 1000));
  }

  console.log("\n登录超时，请重启应用重试。");
  process.exit(1);
}

main().catch((err) => {
  console.error(`错误: ${err}`);
  process.exit(1);
});
