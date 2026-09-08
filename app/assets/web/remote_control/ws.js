const BACKOFF_STEPS = [500, 1000, 2000, 4000, 8000];
const HEARTBEAT_MS = 15000;
const PONG_DEADLINE_MS = 8000;
const RESUME_PONG_DEADLINE_MS = 3000;
const CONNECT_TIMEOUT_MS = 10000;
const MIN_OPEN_INTERVAL_MS = 1000;
const DEFAULT_TIMEOUT_MS = 30000;

export function connect(handlers) {
  const pending = new Map();
  const binaryHandlers = new Map();
  let socket = null;
  let attempt = 0;
  let heartbeat = null;
  let pongTimer = null;
  let connectTimer = null;
  let reconnectTimer = null;
  let lastOpenAt = 0;
  let stopped = false;
  let listening = false;
  let nextId = 1;

  function url() {
    const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${scheme}//${window.location.host}/api/v1/ws`;
  }

  function setStatus(status, extra) {
    handlers.onStatus({ status, ...(extra || {}) });
  }

  function clearReconnectTimer() {
    if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function clearPongTimer() {
    if (pongTimer !== null) window.clearTimeout(pongTimer);
    pongTimer = null;
  }

  function clearConnectTimer() {
    if (connectTimer !== null) window.clearTimeout(connectTimer);
    connectTimer = null;
  }

  function open() {
    if (stopped) return;
    clearReconnectTimer();
    discardSocket();
    lastOpenAt = Date.now();
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
    connectTimer = window.setTimeout(() => {
      connectTimer = null;
      if (socket === next) dropSocket();
    }, CONNECT_TIMEOUT_MS);

    next.addEventListener('open', () => {
      if (socket !== next) return;
      attempt = 0;
      clearConnectTimer();
      startHeartbeat();
      setStatus('open');
    });

    next.addEventListener('message', (event) => {
      if (socket !== next) return;
      clearPongTimer();
      if (typeof event.data === 'string') {
        handleText(event.data);
      } else {
        handleBinary(new Uint8Array(event.data));
      }
    });

    next.addEventListener('close', (event) => {
      if (socket !== next) return;
      releaseSocket();
      failAllPending({ code: 'disconnected', message: 'The connection dropped.' });
      if (stopped) {
        setStatus('closed', { code: event.code });
        return;
      }
      scheduleReconnect();
    });

    next.addEventListener('error', () => {
      if (socket !== next) return;
      setStatus('error');
    });
  }

  function releaseSocket() {
    socket = null;
    stopHeartbeat();
    clearPongTimer();
    clearConnectTimer();
  }

  function discardSocket() {
    const stale = socket;
    if (!stale) return;
    releaseSocket();
    try {
      stale.close();
    } catch (error) {
      return;
    }
  }

  function dropSocket() {
    discardSocket();
    failAllPending({ code: 'disconnected', message: 'The connection stalled.' });
    if (stopped) return;
    scheduleReconnect();
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

  function sendPing(deadlineMs) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    try {
      socket.send(JSON.stringify({ type: 'ping', ts: Date.now() }));
    } catch (error) {
      dropSocket();
      return;
    }
    if (pongTimer !== null) return;
    pongTimer = window.setTimeout(() => {
      pongTimer = null;
      dropSocket();
    }, deadlineMs);
  }

  function startHeartbeat() {
    stopHeartbeat();
    heartbeat = window.setInterval(() => {
      if (document.hidden) return;
      sendPing(PONG_DEADLINE_MS);
    }, HEARTBEAT_MS);
  }

  function stopHeartbeat() {
    if (heartbeat !== null) {
      window.clearInterval(heartbeat);
      heartbeat = null;
    }
  }

  function scheduleReconnect() {
    clearReconnectTimer();
    const base = BACKOFF_STEPS[Math.min(attempt, BACKOFF_STEPS.length - 1)];
    const jitter = base * 0.2 * (Math.random() * 2 - 1);
    const delay = Math.max(250, Math.round(base + jitter));
    attempt += 1;
    setStatus('reconnecting', { reconnectAt: Date.now() + delay });
    reconnectTimer = window.setTimeout(open, delay);
  }

  function openSoon() {
    if (stopped) return;
    const wait = Math.max(0, lastOpenAt + MIN_OPEN_INTERVAL_MS - Date.now());
    if (wait === 0) {
      open();
      return;
    }
    clearReconnectTimer();
    setStatus('reconnecting', { reconnectAt: Date.now() + wait });
    reconnectTimer = window.setTimeout(open, wait);
  }

  function ensureConnecting() {
    if (stopped) return;
    if (socket && socket.readyState === WebSocket.CONNECTING) return;
    if (socket && socket.readyState === WebSocket.OPEN) return;
    openSoon();
  }

  function resume() {
    if (stopped) return;
    attempt = 0;
    if (socket && socket.readyState === WebSocket.OPEN) {
      sendPing(RESUME_PONG_DEADLINE_MS);
      return;
    }
    if (socket && socket.readyState === WebSocket.CONNECTING) return;
    clearReconnectTimer();
    openSoon();
  }

  function listen() {
    if (listening) return;
    listening = true;
    window.addEventListener('online', resume);
    window.addEventListener('pageshow', resume);
    window.addEventListener('focus', resume);
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) resume();
    });
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
      listen();
      open();
    },
    retry() {
      stopped = false;
      attempt = 0;
      open();
    },
    resume,
    stop(reason) {
      stopped = true;
      clearReconnectTimer();
      discardSocket();
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
          ensureConnecting();
          reject({ code: 'disconnected', message: 'Not connected to Spirit.' });
          return;
        }
        const timer = window.setTimeout(() => {
          pending.delete(id);
          reject({ code: 'timeout', message: 'Spirit did not answer in time.' });
        }, timeoutMs);
        pending.set(id, { resolve, reject, timer });
        try {
          socket.send(JSON.stringify({ type: 'command', id, name, params: params || {} }));
        } catch (error) {
          dropSocket();
        }
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
