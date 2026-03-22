import { spawn } from "node:child_process";
import https from "node:https";
import http from "node:http";
import type { GlobalConfig, NotificationChannelConfig } from "./types.js";

export interface NotificationPayload {
  event: string;
  project: string;
  taskId: string;
  message: string;
  timestamp: string;
  [key: string]: unknown;
}

/**
 * Fire notifications through all configured channels.
 * Shell-command hooks (legacy) are handled separately in the worker.
 */
export async function fireNotifications(
  config: GlobalConfig,
  payload: NotificationPayload,
): Promise<void> {
  const channels: NotificationChannelConfig[] = config.notificationChannels ?? [];
  const promises: Promise<void>[] = [];

  for (const channel of channels) {
    if (channel.events.length > 0 && !channel.events.includes(payload.event) && !channel.events.includes("*")) {
      continue;
    }

    if (channel.type === "webhook") {
      promises.push(sendWebhook(channel.url, payload));
    } else if (channel.type === "desktop") {
      promises.push(sendDesktopNotification(payload));
    }
  }

  await Promise.allSettled(promises);
}

async function sendWebhook(url: string, payload: NotificationPayload): Promise<void> {
  const parsedUrl = new URL(url);
  const transport = parsedUrl.protocol === "https:" ? https : http;
  const body = JSON.stringify(payload);

  return new Promise<void>((resolve) => {
    const req = transport.request(
      parsedUrl,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(body),
          "User-Agent": "openloop-daemon/1.0",
        },
        timeout: 10_000,
      },
      (res) => {
        // Drain the response body
        res.resume();
        res.on("end", () => resolve());
      },
    );
    req.on("error", () => resolve());
    req.on("timeout", () => {
      req.destroy();
      resolve();
    });
    req.write(body);
    req.end();
  });
}

async function sendDesktopNotification(payload: NotificationPayload): Promise<void> {
  const title = `OpenLoop: ${payload.event}`;
  const body = payload.message;

  const platform = process.platform;
  let command: string;
  let args: string[];

  if (platform === "linux") {
    command = "notify-send";
    args = [title, body];
  } else if (platform === "darwin") {
    command = "osascript";
    args = ["-e", `display notification "${body.replace(/"/g, '\\"')}" with title "${title.replace(/"/g, '\\"')}"`];
  } else {
    // Unsupported platform for desktop notifications
    return;
  }

  return new Promise<void>((resolve) => {
    const child = spawn(command, args, { stdio: "ignore" });
    child.on("close", () => resolve());
    child.on("error", () => resolve());
  });
}
