const BACKOFF_STEPS = [500, 1000, 2000, 4000, 8000];
const HEARTBEAT_MS = 15000;
const DEFAULT_TIMEOUT_MS = 30000;

export function connect(handlers) {
  const pending = new Map();
  const binaryHandlers = new Map();
  let socket = null;
  let attempt = 0;
  let heartbeat = null;
  let reconnectTimer = null;
  let stopped = false;
  let nextId = 1;

  function url() {
    const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${scheme}//${window.location.host}/api/v1/ws`;
  }

  function setStatus(status, extra) {
    handlers.onStatus({ status, ...(extra || {}) });
  }

  function open() {
    if (stopped) return;
    setStatus(attempt === 0 ? 'connecting' : 'reconnecting');
    let next;
    try {
      next = new WebSocket(url());
    } catch (error) {
      scheduleReconnect();
      return;
    }
    next.binaryType = 'arraybuffer';
    socket = next;

    next.addEventListener('open', () => {
      attempt = 0;
      startHeartbeat();
      setStatus('open');
    });

    next.addEventListener('message', (event) => {
      if (typeof event.data === 'string') {
        handleText(event.data);
      } else {
        handleBinary(new Uint8Array(event.data));
      }
    });

    next.addEventListener('close', (event) => {
      stopHeartbeat();
      socket = null;
      failAllPending({ code: 'disconnected', message: 'The connection dropped.' });
      if (stopped) {
        setStatus('closed', { code: event.code });
        return;
      }
      scheduleReconnect();
    });

    next.addEventListener('error', () => {
      setStatus('error');
    });
  }

  function handleText(raw) {
    let message;
    try {
      message = JSON.parse(raw);
    } catch (error) {
      return;
    }
    switch (message.type) {
      case 'hello':
        handlers.onHello(message);
        break;
      case 'state':
        handlers.onState(message);
        break;
      case 'result': {
        const entry = pending.get(message.id);
        if (!entry) return;
        pending.delete(message.id);
        window.clearTimeout(entry.timer);
        if (message.ok) entry.resolve(message.data || {});
        else entry.reject(message.error || { code: 'invalid_request', message: 'Command failed.' });
        break;
      }
      case 'event':
        handlers.onEvent(message);
        break;
      case 'pong':
        break;
      default:
        break;
    }
  }

  function handleBinary(bytes) {
    if (bytes.length < 4) return;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const attachId = view.getUint32(0, true);
    const handler = binaryHandlers.get(attachId);
    if (handler) handler(bytes.subarray(4));
  }

  function startHeartbeat() {
    stopHeartbeat();
    heartbeat = window.setInterval(() => {
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: 'ping', ts: Date.now() }));
      }
    }, HEARTBEAT_MS);
  }

  function stopHeartbeat() {
    if (heartbeat !== null) {
      window.clearInterval(heartbeat);
      heartbeat = null;
    }
  }

  function scheduleReconnect() {
    const base = BACKOFF_STEPS[Math.min(attempt, BACKOFF_STEPS.length - 1)];
    const jitter = base * 0.2 * (Math.random() * 2 - 1);
    const delay = Math.max(250, Math.round(base + jitter));
    attempt += 1;
    setStatus('reconnecting', { reconnectAt: Date.now() + delay });
    reconnectTimer = window.setTimeout(open, delay);
  }

  function failAllPending(error) {
    for (const entry of pending.values()) {
      window.clearTimeout(entry.timer);
      entry.reject(error);
    }
    pending.clear();
  }

  return {
    start() {
      stopped = false;
      open();
    },
    retry() {
      stopped = false;
      attempt = 0;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      open();
    },
    stop(reason) {
      stopped = true;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      stopHeartbeat();
      if (socket) socket.close();
      socket = null;
      setStatus('closed', { reason });
    },
    isOpen() {
      return Boolean(socket) && socket.readyState === WebSocket.OPEN;
    },
    command(name, params, options) {
      const id = String(nextId++);
      const timeoutMs = (options && options.timeoutMs) || DEFAULT_TIMEOUT_MS;
      return new Promise((resolve, reject) => {
        if (!socket || socket.readyState !== WebSocket.OPEN) {
          reject({ code: 'disconnected', message: 'Not connected to Spirit.' });
          return;
        }
        const timer = window.setTimeout(() => {
          pending.delete(id);
          reject({ code: 'timeout', message: 'Spirit did not answer in time.' });
        }, timeoutMs);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ type: 'command', id, name, params: params || {} }));
      });
    },
    sendBinary(attachId, bytes) {
      if (!socket || socket.readyState !== WebSocket.OPEN) return false;
      const frame = new Uint8Array(4 + bytes.length);
      new DataView(frame.buffer).setUint32(0, attachId, true);
      frame.set(bytes, 4);
      socket.send(frame);
      return true;
    },
    onBinary(attachId, handler) {
      binaryHandlers.set(attachId, handler);
      return () => binaryHandlers.delete(attachId);
    },
    clearBinary(attachId) {
      binaryHandlers.delete(attachId);
    },
  };
}

export function decodeBase64(value) {
  const binary = window.atob(value || '');
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

export function encodeBase64(bytes) {
  let binary = '';
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index]);
  }
  return window.btoa(binary);
}
