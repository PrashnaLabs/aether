import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import fs from "node:fs";
import net from "node:net";

type Spec = { name: string; description: string; inputSchema: Record<string, unknown> };

// windows: the sock file holds "host:port" of the loopback listener
function connectTcp(file: string) {
  const addr = fs.readFileSync(file, "utf8").trim();
  const i = addr.lastIndexOf(":");
  return net.createConnection({ host: addr.slice(0, i), port: Number(addr.slice(i + 1)) });
}

function ask(sock: string, line: string, signal?: AbortSignal): Promise<string> {
  return new Promise((resolve, reject) => {
    let c: net.Socket;
    try {
      c = process.platform === "win32" ? connectTcp(sock) : net.createConnection(sock);
    } catch {
      return reject(new Error("Aether isn't running"));
    }
    c.setEncoding("utf8");
    let buf = "";
    let connected = false;
    const abort = () => c.destroy(new Error("Stopped"));
    signal?.addEventListener("abort", abort, { once: true });
    c.on("connect", () => {
      connected = true;
      c.write(line + "\n");
    });
    c.on("data", (d) => {
      buf += d;
      if (buf.includes("\n")) c.end();
    });
    c.on("error", (e) => reject(connected ? e : new Error("Aether isn't running")));
    c.on("close", () => {
      signal?.removeEventListener("abort", abort);
      resolve(buf.split("\n")[0]);
    });
  });
}

export default function (pi: ExtensionAPI) {
  const sock = process.env.AETHER_ORCH_SOCK;
  const token = process.env.AETHER_ORCH_TOKEN;
  const rawSpecs = process.env.AETHER_ORCH_SPECS;
  delete process.env.AETHER_ORCH_SOCK;
  delete process.env.AETHER_ORCH_TOKEN;
  delete process.env.AETHER_ORCH_SPECS;
  if (!sock || !token) return;
  let specs: Spec[];
  try {
    specs = JSON.parse(rawSpecs || "[]");
  } catch {
    return;
  }
  for (const spec of specs) {
    pi.registerTool({
      name: `aether_${spec.name}`,
      label: `Aether ${spec.name.replace(/_/g, " ")}`,
      description: spec.description,
      parameters: spec.inputSchema as any,
      async execute(_id, params, signal) {
        const line = await ask(sock, JSON.stringify({ token, tool: spec.name, args: params ?? {} }), signal);
        let reply: { ok?: boolean; result?: unknown; error?: string };
        try {
          reply = JSON.parse(line);
        } catch {
          throw new Error("Aether didn't answer");
        }
        if (reply.ok !== true) throw new Error(reply.error || "Aether didn't answer");
        return { content: [{ type: "text", text: JSON.stringify(reply.result) }], details: {} };
      },
    });
  }
}
