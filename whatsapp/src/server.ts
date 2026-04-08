
import { serve } from "hono";
import { Hono } from "hono";
import { WhatsAppIntegration } from "./index";

const app = new Hono();
const wa = new WhatsAppIntegration({
  authDir: process.env.WHATSAPP_AUTH_DIR ?? "./auth_whatsapp",
});

const GATEWAY_URL = process.env.AUXLOCLAW_GATEWAY_URL ?? "http://localhost:18789";

// Initialize WhatsApp
await wa.connect();

// Forward incoming messages to Rust gateway
wa.onMessage(async (msg) => {
  const text = msg.message.conversation || msg.message.extendedTextMessage?.text || "";
  if (!text) return;

  console.log(`Forwarding message from ${msg.pushName} (${msg.key.remoteJid}) to gateway...`);
  
  try {
    await fetch(`${GATEWAY_URL}/api/whatsapp/message`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jid: msg.key.remoteJid,
        pushName: msg.pushName,
        text: text,
      }),
    });
  } catch (error) {
    console.error("Failed to forward message to gateway:", error);
  }
});

// API Endpoints
app.post("/send", async (c) => {
  const { jid, text } = await c.req.json();
  try {
    await wa.sendMessage(jid, text);
    return c.json({ success: true });
  } catch (error: any) {
    return c.json({ success: false, error: error.message }, 500);
  }
});

app.post("/pairing-code", async (c) => {
  const { phoneNumber } = await c.req.json();
  try {
    const code = await wa.requestPairingCode(phoneNumber);
    return c.json({ code });
  } catch (error: any) {
    return c.json({ error: error.message }, 500);
  }
});

app.get("/status", (c) => {
  return c.json(wa.getState());
});

const port = 18790;
console.log(`🚀 WhatsApp Bridge listening on port ${port}`);
console.log(`🔗 Gateway URL: ${GATEWAY_URL}`);
serve({
  fetch: app.fetch,
  port: port,
});
