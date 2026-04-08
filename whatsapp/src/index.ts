/**
 * WhatsApp Integration with Pairing Code Flow
 * Uses Baileys (WhiskeySockets) for WhatsApp Web protocol
 */

import makeWASocket, {
  useMultiFileAuthState,
  DisconnectReason,
  proto,
  WASocket,
} from "@whiskeysockets/baileys";
import { Boom } from "@hapi/boom";

export interface WhatsAppConfig {
  authDir: string;
  browser: [string, string, string];
  printQR: boolean;
}

export interface WhatsAppMessage {
  key: {
    remoteJid: string;
    fromMe: boolean;
    id: string;
  };
  message: any;
  pushName: string;
  timestamp: number;
}

export interface ConnectionState {
  connected: boolean;
  pairingCode: string | null;
  qrCode: string | null;
  error: string | null;
}

export class WhatsAppIntegration {
  private sock: WASocket | null = null;
  private config: WhatsAppConfig;
  private state: ConnectionState = {
    connected: false,
    pairingCode: null,
    qrCode: null,
    error: null,
  };
  private messageHandlers: ((msg: WhatsAppMessage) => void)[] = [];
  private connectionHandlers: ((state: ConnectionState) => void)[] = [];

  constructor(config: Partial<WhatsAppConfig> = {}) {
    this.config = {
      authDir: config.authDir ?? "./auth_whatsapp",
      browser: config.browser ?? ["Chrome (Windows)", "", ""],
      printQR: config.printQR ?? false,
    };
  }

  /**
   * Initialize the WhatsApp socket with pairing code flow
   */
  async connect(): Promise<ConnectionState> {
    return new Promise(async (resolve) => {
      try {
        // Load existing auth state
        const { state, saveCreds } = await useMultiFileAuthState(
          this.config.authDir
        );

        // Create socket with QR disabled (we use pairing code instead)
        this.sock = makeWASocket({
          auth: state,
          printQRInTerminal: this.config.printQR,
          browser: this.config.browser,
        });

        // Save credentials when updated
        this.sock.ev.on("creds.update", saveCreds);

        // Handle connection updates
        this.sock.ev.on(
          "connection.update",
          ({ connection, lastDisconnect }) => {
            this.handleConnectionUpdate(connection, lastDisconnect);
            
            // Resolve when connected
            if (connection === "open") {
              resolve(this.state);
            }
          }
        );

        // Handle incoming messages
        this.sock.ev.on("messages.upsert", ({ messages }) => {
          this.handleIncomingMessages(messages);
        });

        // If already registered (from previous session), we're done
        if (this.sock.authState.creds.registered) {
          this.state.connected = true;
          resolve(this.state);
        }
      } catch (error) {
        this.state.error = (error as Error).message;
        this.notifyConnectionHandlers();
        resolve(this.state);
      }
    });
  }

  /**
   * Connect and request pairing code in sequence
   */
  async connectWithPairing(phoneNumber: string): Promise<string> {
    // Load existing auth state
    const { state, saveCreds } = await useMultiFileAuthState(
      this.config.authDir
    );

    // Create socket
    this.sock = makeWASocket({
      auth: state,
      printQRInTerminal: this.config.printQR,
      browser: this.config.browser,
    });

    // Save credentials when updated
    this.sock.ev.on("creds.update", saveCreds);

    // Request pairing code immediately when socket opens
    this.sock.ev.on("socket.open", async () => {
      console.log("Socket opened, requesting pairing code...");
      try {
        const cleanNumber = phoneNumber.replace(/\D/g, "");
        const code = await this.sock!.requestPairingCode(cleanNumber);
        const formattedCode = code.slice(0, 4) + "-" + code.slice(4);
        this.state.pairingCode = formattedCode;
        console.log("\n=================================");
        console.log(`PAIRING CODE: ${formattedCode}`);
        console.log("=================================");
        console.log("\n1. Open WhatsApp on your phone");
        console.log("2. Go to Settings > Linked Devices");
        console.log("3. Tap 'Link with phone number instead'");
        console.log("4. Enter the code above\n");
      } catch (error) {
        console.error("Failed to get pairing code:", error);
      }
    });

    // Handle connection updates
    this.sock.ev.on(
      "connection.update",
      ({ connection, lastDisconnect }) => {
        this.handleConnectionUpdate(connection, lastDisconnect);
      }
    );

    // Handle incoming messages
    this.sock.ev.on("messages.upsert", ({ messages }) => {
      this.handleIncomingMessages(messages);
    });

    return this.state.pairingCode ?? "pending";
  }

  /**
   * Request a pairing code for the given phone number
   * @param phoneNumber - Phone number in international format without + (e.g., "2348030000000")
   */
  async requestPairingCode(phoneNumber: string): Promise<string> {
    if (!this.sock) {
      throw new Error("Socket not initialized. Call connect() first.");
    }

    // Sanitize: remove all non-digits
    const cleanNumber = phoneNumber.replace(/\D/g, "");

    if (cleanNumber.length < 10) {
      throw new Error(
        "Invalid phone number. Must be at least 10 digits with country code."
      );
    }

    try {
      // Request the pairing code from WhatsApp
      const code = await this.sock.requestPairingCode(cleanNumber);

      // Format code with hyphen (e.g., "J2KX-9LPN")
      const formattedCode = code.slice(0, 4) + "-" + code.slice(4);

      this.state.pairingCode = formattedCode;
      this.notifyConnectionHandlers();

      console.log(`Pairing code: ${formattedCode}`);
      return formattedCode;
    } catch (error) {
      this.state.error = (error as Error).message;
      this.notifyConnectionHandlers();
      throw error;
    }
  }

  /**
   * Check if the device is already paired
   */
  isRegistered(): boolean {
    return this.sock?.authState.creds.registered ?? false;
  }

  /**
   * Get current connection state
   */
  getState(): ConnectionState {
    return { ...this.state };
  }

  /**
   * Disconnect from WhatsApp
   */
  async disconnect(): Promise<void> {
    if (this.sock) {
      this.sock.end(undefined);
      this.sock = null;
      this.state = {
        connected: false,
        pairingCode: null,
        qrCode: null,
        error: null,
      };
      this.notifyConnectionHandlers();
    }
  }

  /**
   * Send a text message
   * @param jid - Recipient's JID (e.g., "2348030000000@s.whatsapp.net")
   * @param text - Message text
   */
  async sendMessage(jid: string, text: string): Promise<any> {
    if (!this.sock) {
      throw new Error("Socket not initialized");
    }

    const cleanJid = jid.includes("@")
      ? jid
      : `${jid.replace(/\D/g, "")}@s.whatsapp.net`;

    return await this.sock.sendMessage(cleanJid, { text });
  }

  /**
   * Send a reply to a message
   */
  async replyMessage(
    jid: string,
    text: string,
    quotedKey: { remoteJid: string; fromMe: boolean; id: string }
  ): Promise<any> {
    if (!this.sock) {
      throw new Error("Socket not initialized");
    }

    return await this.sock.sendMessage(jid, { text }, { quoted: quotedKey });
  }

  /**
   * Register a callback for incoming messages
   */
  onMessage(handler: (msg: WhatsAppMessage) => void): void {
    this.messageHandlers.push(handler);
  }

  /**
   * Register a callback for connection state changes
   */
  onConnectionChange(handler: (state: ConnectionState) => void): void {
    this.connectionHandlers.push(handler);
  }

  private handleConnectionUpdate(
    connection: string,
    lastDisconnect?: { error: Boom; date: Date }
  ): void {
    if (connection === "open") {
      this.state.connected = true;
      this.state.error = null;
      this.state.pairingCode = null;
      console.log("WhatsApp connected successfully!");
    } else if (connection === "close") {
      this.state.connected = false;

      const shouldReconnect =
        lastDisconnect?.error?.output?.statusCode !== DisconnectReason.loggedOut;

      if (shouldReconnect) {
        console.log("Connection closed, will attempt to reconnect...");
        this.connect();
      } else {
        console.log("Logged out from WhatsApp");
        this.state.error = "Logged out";
      }
    }

    this.notifyConnectionHandlers();
  }

  private handleIncomingMessages(messages: proto.IWebMessageInfo[]): void {
    for (const msg of messages) {
      if (!msg.key.fromMe && msg.message) {
        const waMessage: WhatsAppMessage = {
          key: {
            remoteJid: msg.key.remoteJid ?? "",
            fromMe: msg.key.fromMe,
            id: msg.key.id ?? "",
          },
          message: msg.message,
          pushName: msg.pushName ?? "Unknown",
          timestamp: msg.messageTimestamp ?? Date.now(),
        };

        for (const handler of this.messageHandlers) {
          handler(waMessage);
        }
      }
    }
  }

  private notifyConnectionHandlers(): void {
    const state = { ...this.state };
    for (const handler of this.connectionHandlers) {
      handler(state);
    }
  }
}

// Export for use as CLI tool
export async function main() {
  const phone = process.argv[2];

  if (!phone) {
    console.log("Usage: bun run index.ts <phone_number>");
    console.log("Example: bun run index.ts 2348030000000");
    return;
  }

  const wa = new WhatsAppIntegration({
    authDir: process.env.WHATSAPP_AUTH_DIR ?? "./auth_whatsapp",
  });

  console.log("Connecting to WhatsApp...");
  await wa.connectWithPairing(phone);

  // Listen for messages
  wa.onMessage((msg) => {
    const text =
      msg.message.conversation ||
      msg.message.extendedTextMessage?.text ||
      "";
    console.log(`[${msg.pushName}]: ${text}`);
  });

  // Listen for connection changes
  wa.onConnectionChange((state) => {
    if (state.connected) {
      console.log("✅ WhatsApp connected and ready!");
    }
  });

  // Keep the process running
  process.stdin.resume();
}

main().catch(console.error);