import { WS_URL, type StreamCursor, type StreamMessage } from "./types";

export type StreamHandlers = {
  onSnapshot: (msg: StreamMessage) => void;
  onDelta: (msg: StreamMessage) => void;
  onHeartbeat: (msg: StreamMessage) => void;
  onEvent: (msg: StreamMessage) => void;
  onResyncRequired: (msg: StreamMessage) => void;
  onError: (msg: StreamMessage) => void;
  onConnectionChange: (connected: boolean) => void;
  onMessage: (msg: StreamMessage) => void;
};

function seqBig(s: string): bigint | null {
  try {
    return BigInt(s);
  } catch {
    return null;
  }
}

export function sequenceGap(prev: string | null, next: string): boolean {
  if (prev === null) return false;
  const a = seqBig(prev);
  const b = seqBig(next);
  if (a === null || b === null) return true;
  return b !== a + 1n;
}

export function cursorFrom(epoch: string, sequence: string | number): StreamCursor {
  return { epoch, sequence: typeof sequence === "number" ? String(sequence) : sequence };
}

export function startStream(handlers: StreamHandlers, topics = ["radar", "system"]): () => void {
  let ws: WebSocket | null = null;
  let stopped = false;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let lastEpoch: string | null = null;
  let lastSeq: string | null = null;

  const connect = () => {
    if (stopped) return;
    ws = new WebSocket(WS_URL);
    ws.onopen = () => {
      attempt = 0;
      handlers.onConnectionChange(true);
      const resume =
        lastEpoch && lastSeq ? { resume: { epoch: lastEpoch, sequence: lastSeq } } : {};
      ws?.send(
        JSON.stringify({
          type: "subscribe",
          request_id: "r1",
          topics,
          ...resume,
        }),
      );
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      let msg: StreamMessage;
      try {
        msg = JSON.parse(ev.data) as StreamMessage;
      } catch {
        return;
      }
      handlers.onMessage(msg);
      if (lastEpoch && msg.epoch && msg.epoch !== lastEpoch) {
        lastEpoch = msg.epoch;
        lastSeq = msg.sequence ?? null;
        handlers.onResyncRequired(msg);
        return;
      }
      if (lastSeq !== null && msg.sequence && sequenceGap(lastSeq, msg.sequence) && msg.type !== "heartbeat") {
        lastEpoch = msg.epoch ?? lastEpoch;
        lastSeq = msg.sequence;
        handlers.onResyncRequired(msg);
        return;
      }
      lastEpoch = msg.epoch ?? lastEpoch;
      if (msg.sequence) lastSeq = msg.sequence;
      switch (msg.type) {
        case "snapshot":
          handlers.onSnapshot(msg);
          break;
        case "delta":
          handlers.onDelta(msg);
          break;
        case "heartbeat":
          handlers.onHeartbeat(msg);
          break;
        case "event":
          handlers.onEvent(msg);
          break;
        case "resync_required":
          handlers.onResyncRequired(msg);
          break;
        case "error":
          handlers.onError(msg);
          break;
        default:
          break;
      }
    };
    ws.onerror = () => {
      /* onclose handles retry */
    };
    ws.onclose = () => {
      handlers.onConnectionChange(false);
      if (stopped) return;
      const delay = Math.min(8000, 400 * 2 ** attempt) + Math.floor(Math.random() * 200);
      attempt += 1;
      timer = setTimeout(connect, delay);
    };
  };

  connect();

  return () => {
    stopped = true;
    if (timer) clearTimeout(timer);
    ws?.close();
    ws = null;
  };
}

export function connectStream(h: {
  onMessage: (m: StreamMessage) => void;
  onStatus: (s: "connected" | "disconnected" | "resync") => void;
}): () => void {
  return startStream({
    onSnapshot: h.onMessage,
    onDelta: h.onMessage,
    onHeartbeat: h.onMessage,
    onEvent: h.onMessage,
    onResyncRequired: (m) => {
      h.onStatus("resync");
      h.onMessage(m);
    },
    onError: h.onMessage,
    onConnectionChange: (c) => h.onStatus(c ? "connected" : "disconnected"),
    onMessage: h.onMessage,
  });
}
