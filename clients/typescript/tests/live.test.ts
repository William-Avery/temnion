// SPDX-License-Identifier: AGPL-3.0-only
import test from "node:test";
import assert from "node:assert/strict";
import * as os from "node:os";
import * as path from "node:path";
import * as fs from "node:fs";
import * as net from "node:net";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { TemnionClient } from "../src/client.js";
import { QueryFormat } from "../src/types.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function getFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = (s.address() as net.AddressInfo).port;
      s.close(() => resolve(port));
    });
    s.on("error", reject);
  });
}

test("Live TypeScript client integration with temniond process", async (t) => {
  const repoRoot = path.resolve(__dirname, "../../../..");
  const exeSuffix = process.platform === "win32" ? ".exe" : "";
  let daemonBin = path.join(repoRoot, "target", "debug", `temniond${exeSuffix}`);
  if (!fs.existsSync(daemonBin)) {
    daemonBin = path.join(repoRoot, "target", "release", `temniond${exeSuffix}`);
  }

  if (!fs.existsSync(daemonBin)) {
    t.skip(`temniond binary not found at ${daemonBin}`);
    return;
  }

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "temnion-ts-test-"));
  const port = await getFreePort();

  const proc = spawn(daemonBin, ["run", "--data-dir", tmpDir, "--bind", `127.0.0.1:${port}`], {
    stdio: "ignore",
  });

  try {
    // Wait for daemon to become ready
    let connected = false;
    for (let i = 0; i < 30; i++) {
      try {
        await new Promise<void>((resolve, reject) => {
          const s = net.createConnection(port, "127.0.0.1", () => {
            s.destroy();
            resolve();
          });
          s.on("error", reject);
        });
        connected = true;
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 100));
      }
    }

    assert.ok(connected, "Failed to connect to temniond socket within 3s");

    const client = new TemnionClient({
      host: "127.0.0.1",
      port,
    });

    await client.connect();

    // 1. Ping
    const latency = await client.ping();
    assert.ok(latency >= 0);
    assert.ok(latency < 500);

    // 2. Describe
    const desc = await client.describe();
    assert.ok(desc.serverId.length > 0);
    assert.equal(desc.version, 1);
    assert.ok(desc.capabilities.includes("tnp"));
    assert.ok(desc.capabilities.includes("live-subscription"));

    // 3. Query
    const res = await client.query("SELECT * FROM events", { format: QueryFormat.Sql });
    assert.equal(res.rowsCount, 0);
    assert.equal(res.truncated, false);

    await client.close();
  } finally {
    proc.kill();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
