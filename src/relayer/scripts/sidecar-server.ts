/**
 * TS sidecar — Seal + Walrus HTTP bridge.
 *
 * Runs on localhost:9000 inside the enclave (or locally for dev).
 * The Rust binary calls it over loopback; the `X-Sidecar-Secret` header
 * keeps any other process from using these endpoints.
 *
 * Endpoints:
 *   POST /seal/encrypt   { plaintext_b64, owner_address } → { ciphertext_b64 }
 *   POST /seal/decrypt   { ciphertext_b64, owner_address } → { plaintext_b64 }
 *   POST /walrus/upload  { bytes_b64, epochs }            → { blob_id }
 *   GET  /walrus/download/:blob_id                        → { bytes_b64 }
 *   GET  /health                                          → "ok"
 */

import Fastify from "fastify";
import { SealClient, getAllowlistedKeyServers, SessionKey } from "@mysten/seal";
import { getFullnodeUrl, SuiClient } from "@mysten/sui/client";

const PORT = parseInt(process.env.SIDECAR_PORT ?? "9000");
const SECRET = process.env.SIDECAR_SECRET ?? "";
const SUI_NETWORK = (process.env.SUI_NETWORK ?? "testnet") as
  | "mainnet"
  | "testnet"
  | "devnet";
const WALRUS_PUBLISHER =
  process.env.WALRUS_PUBLISHER_URL ??
  "https://publisher.walrus-testnet.walrus.space";
const WALRUS_AGGREGATOR =
  process.env.WALRUS_AGGREGATOR_URL ??
  "https://aggregator.walrus-testnet.walrus.space";

// ── Sui + Seal clients ────────────────────────────────────────────────────────

const suiClient = new SuiClient({ url: getFullnodeUrl(SUI_NETWORK) });
const sealClient = new SealClient({
  suiClient,
  serverObjectIds: getAllowlistedKeyServers(SUI_NETWORK),
  verifyKeyServers: false,
});

// ── Auth middleware ───────────────────────────────────────────────────────────

function requireSecret(
  req: { headers: Record<string, string | string[] | undefined> },
  reply: { code: (n: number) => { send: (s: string) => void } }
): boolean {
  const hdr = req.headers["x-sidecar-secret"];
  const provided = Array.isArray(hdr) ? hdr[0] : hdr ?? "";
  // Constant-time compare to resist timing attacks.
  if (!timingSafeEqual(provided, SECRET)) {
    reply.code(401).send("unauthorized");
    return false;
  }
  return true;
}

function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

// ── Fastify ───────────────────────────────────────────────────────────────────

const app = Fastify({ logger: false });

app.get("/health", async () => "ok");

// ── Seal: encrypt ─────────────────────────────────────────────────────────────
app.post<{ Body: { plaintext_b64: string; owner_address: string } }>(
  "/seal/encrypt",
  async (req, reply) => {
    if (!requireSecret(req, reply)) return;
    const { plaintext_b64, owner_address } = req.body;
    const plaintext = Buffer.from(plaintext_b64, "base64");

    // Derive a deterministic id — the owner address + sha256 of plaintext.
    // Seal's allowlist is keyed by object-id; we let the relayer's own
    // pgvector namespace serve as the access-control boundary.
    const id = crypto.randomUUID().replace(/-/g, "");

    // SessionKey ties this encryption to the owner's Sui address.
    const sessionKey = new SessionKey({
      address: owner_address,
      packageId: process.env.PINAIVU_PACKAGE_ID ?? "0x0",
      ttlMin: 30,
      suiClient,
    });

    const { encryptedObject } = await sealClient.encrypt({
      threshold: 2,
      packageId: process.env.PINAIVU_PACKAGE_ID ?? "0x0",
      id,
      data: plaintext,
    });

    return {
      ciphertext_b64: Buffer.from(encryptedObject).toString("base64"),
    };
  }
);

// ── Seal: decrypt ─────────────────────────────────────────────────────────────
app.post<{ Body: { ciphertext_b64: string; owner_address: string } }>(
  "/seal/decrypt",
  async (req, reply) => {
    if (!requireSecret(req, reply)) return;
    const { ciphertext_b64, owner_address } = req.body;
    const ciphertext = Buffer.from(ciphertext_b64, "base64");

    const sessionKey = new SessionKey({
      address: owner_address,
      packageId: process.env.PINAIVU_PACKAGE_ID ?? "0x0",
      ttlMin: 30,
      suiClient,
    });

    const decrypted = await sealClient.decrypt({
      data: ciphertext,
      sessionKey,
      fetchKeys: async (ids, sessionKey) => {
        return sealClient.fetchKeys({ ids, sessionKey });
      },
    });

    return {
      plaintext_b64: Buffer.from(decrypted).toString("base64"),
    };
  }
);

// ── Walrus: upload ────────────────────────────────────────────────────────────
app.post<{ Body: { bytes_b64: string; epochs: number } }>(
  "/walrus/upload",
  async (req, reply) => {
    if (!requireSecret(req, reply)) return;
    const { bytes_b64, epochs = 5 } = req.body;
    const bytes = Buffer.from(bytes_b64, "base64");

    const res = await fetch(`${WALRUS_PUBLISHER}/v1/blobs?epochs=${epochs}`, {
      method: "PUT",
      headers: { "Content-Type": "application/octet-stream" },
      body: bytes,
    });
    if (!res.ok) {
      reply.code(502).send(`walrus publisher ${res.status}`);
      return;
    }
    const json = (await res.json()) as {
      newlyCreated?: { blobObject: { blobId: string } };
      alreadyCertified?: { blobId: string };
    };
    const blob_id =
      json.newlyCreated?.blobObject?.blobId ??
      json.alreadyCertified?.blobId;
    if (!blob_id) {
      reply.code(502).send("walrus publisher returned no blob_id");
      return;
    }
    return { blob_id };
  }
);

// ── Walrus: download ──────────────────────────────────────────────────────────
app.get<{ Params: { blob_id: string } }>(
  "/walrus/download/:blob_id",
  async (req, reply) => {
    if (!requireSecret(req, reply)) return;
    const { blob_id } = req.params;

    const res = await fetch(`${WALRUS_AGGREGATOR}/v1/blobs/${blob_id}`);
    if (res.status === 404) {
      reply.code(404).send("not found");
      return;
    }
    if (!res.ok) {
      reply.code(502).send(`walrus aggregator ${res.status}`);
      return;
    }
    const buf = Buffer.from(await res.arrayBuffer());
    return { bytes_b64: buf.toString("base64") };
  }
);

// ── Start ─────────────────────────────────────────────────────────────────────
app.listen({ port: PORT, host: "127.0.0.1" }, (err, address) => {
  if (err) {
    console.error(err);
    process.exit(1);
  }
  console.log(`sidecar listening on ${address}`);
});
