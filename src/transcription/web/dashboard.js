(() => {
  "use strict";

  const token = location.pathname.split("/").filter(Boolean).at(-1);
  const status = document.querySelector("#status");
  const empty = document.querySelector("#empty");
  const transcripts = document.querySelector("#transcripts");
  const utterances = new Map();
  const partials = new Map();
  const partialTimers = new Map();
  const translatedLanguages = new Set(["ja", "en", "ko"]);
  let events = null;
  let reconnectTimer = null;
  let terminal = false;
  let waitingForVoice = false;
  let lastEventId = "";
  try {
    const saved = JSON.parse(sessionStorage.getItem("rstt:last-event") || "null");
    if (saved?.path === location.pathname && typeof saved.id === "string") {
      lastEventId = saved.id;
    }
  } catch (_) { /* Storage can be unavailable or contain stale data. */ }

  function speakerName(event) {
    return typeof event.speaker === "string" && event.speaker ? event.speaker : "unknown";
  }

  function setStatus(kind, text) {
    status.className = `status ${kind}`;
    status.lastChild.textContent = text;
  }

  function line(className, label, text, lang) {
    const row = document.createElement("p");
    row.className = className;
    if (typeof lang === "string" && lang) row.lang = lang;
    const name = document.createElement("strong");
    name.textContent = label;
    row.append(name, document.createTextNode(text));
    return row;
  }

  function speakerAvatar(event) {
    const fallback = () => {
      const initial = document.createElement("span");
      initial.textContent = speakerName(event).slice(0, 1).toUpperCase();
      return initial;
    };
    if (typeof event.avatar_url !== "string" || !event.avatar_url) return fallback();

    const avatar = document.createElement("img");
    avatar.src = event.avatar_url;
    avatar.alt = "";
    avatar.referrerPolicy = "no-referrer";
    avatar.addEventListener("error", () => avatar.replaceWith(fallback()), { once: true });
    return avatar;
  }

  function utteranceKey(event) {
    return `${String(event.stream_id)}:${String(event.utterance_id)}`;
  }

  function cardFor(event) {
    const key = utteranceKey(event);
    let card = utterances.get(key);
    if (card) return card;

    card = document.createElement("article");
    card.className = "card";
    card.dataset.utterance = key;
    const heading = document.createElement("div");
    heading.className = "speaker";
    const name = document.createElement("strong");
    name.textContent = speakerName(event);
    heading.append(speakerAvatar(event), name);
    card.append(heading);
    transcripts.prepend(card);
    utterances.set(key, card);
    const cards = transcripts.querySelectorAll(".card");
    if (cards.length > 100) {
      const removed = cards[cards.length - 1];
      utterances.delete(removed.dataset.utterance);
      removed.remove();
    }
    empty.hidden = true;
    return card;
  }

  function clearPartial(streamId) {
    clearTimeout(partialTimers.get(streamId));
    partialTimers.delete(streamId);
    partials.get(streamId)?.remove();
    partials.delete(streamId);
    empty.hidden = transcripts.childElementCount !== 0;
  }

  function onPartial(event) {
    let row = partials.get(event.stream_id);
    if (!row) {
      row = document.createElement("div");
      row.className = "partial";
      const heading = document.createElement("div");
      heading.className = "speaker";
      const name = document.createElement("strong");
      name.textContent = speakerName(event);
      heading.append(speakerAvatar(event), name);
      const text = document.createElement("p");
      row.append(heading, text);
      partials.set(event.stream_id, row);
      transcripts.prepend(row);
    }
    row.querySelector("p").textContent = event.text || "";
    clearTimeout(partialTimers.get(event.stream_id));
    partialTimers.set(event.stream_id, setTimeout(() => clearPartial(event.stream_id), 15000));
    empty.hidden = true;
  }

  function onFinal(event) {
    clearPartial(event.stream_id);
    const card = cardFor(event);
    const label = translatedLanguages.has(event.lang) ? event.lang : "原文";
    const source = line("source", `${label} `, event.text || "", event.lang);
    source.dataset.kind = "source";
    card.querySelector('[data-kind="source"]')?.remove();
    card.append(source);
  }

  function onTranslation(event) {
    if (!translatedLanguages.has(event.lang)) return;
    const card = cardFor(event);
    const selector = `[data-lang="${event.lang}"]`;
    const label = `${event.lang} `;
    const translated = line("translation", label, event.text || "", event.lang);
    translated.dataset.lang = event.lang;
    card.querySelector(selector)?.remove();
    card.append(translated);
  }

  function onEvent(event) {
    if (!event || typeof event !== "object" || typeof event.type !== "string") return;
    if (typeof event.text !== "string") return;
    if (event.stream_id === undefined || event.stream_id === null) return;
    if (event.type !== "partial" &&
        (typeof event.utterance_id !== "string" || !event.utterance_id)) return;
    if (event.type === "partial") onPartial(event);
    if (event.type === "final") onFinal(event);
    if (event.type === "translation") onTranslation(event);
  }

  function rememberEventId(event) {
    const candidate = event.lastEventId;
    const parsed = /^([0-9a-fA-F]{32}):(\d+)$/.exec(candidate);
    if (!parsed) return true;
    const current = /^([0-9a-fA-F]{32}):(\d+)$/.exec(lastEventId);
    if (current && current[1] === parsed[1] && BigInt(parsed[2]) <= BigInt(current[2])) {
      return false;
    }
    lastEventId = candidate;
    try {
      sessionStorage.setItem("rstt:last-event", JSON.stringify({ path: location.pathname, id: candidate }));
    } catch (_) { /* Reconnection still works for the lifetime of this page. */ }
    return true;
  }

  function scheduleReconnect(delay = 5000) {
    if (terminal || reconnectTimer !== null) return;
    reconnectTimer = setTimeout(connect, delay);
  }

  async function handleConnectionError(source) {
    if (events !== source) return;
    if (!waitingForVoice) setStatus("offline", "接続確認中");

    let response;
    try {
      response = await fetch(`/api/access/${encodeURIComponent(token)}`, {
        credentials: "same-origin",
        cache: "no-store",
      });
    } catch (_) {
      if (events === source) {
        setStatus("offline", "再接続中");
        scheduleReconnect();
      }
      return;
    }

    if (events !== source || source.readyState === EventSource.OPEN) return;
    if (response.status === 404) {
      terminal = true;
      waitingForVoice = false;
      source.close();
      events = null;
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
      setStatus("offline", "通話終了");
      return;
    }
    if (response.status === 401) {
      waitingForVoice = false;
      source.close();
      setStatus("offline", "再認証が必要");
      if (document.visibilityState === "visible") {
        location.assign(`/view/${encodeURIComponent(token)}`);
      }
      return;
    }
    if (response.status === 403) {
      waitingForVoice = true;
      setStatus("offline", "VCへの再参加を待機中");
      scheduleReconnect();
      return;
    }
    waitingForVoice = false;
    setStatus("offline", "再接続中");
    scheduleReconnect();
  }

  function connect() {
    if (terminal) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
    events?.close();
    const after = lastEventId ? `?after=${encodeURIComponent(lastEventId)}` : "";
    const source = new EventSource(`/api/events/${encodeURIComponent(token)}${after}`);
    events = source;
    if (!waitingForVoice) setStatus("connecting", "接続中");
    source.addEventListener("open", () => {
      if (events !== source) return;
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
      waitingForVoice = false;
      setStatus("online", "ライブ");
    });
    source.addEventListener("keepalive", (event) => {
      if (events === source) rememberEventId(event);
    });
    source.addEventListener("transcript", (event) => {
      if (events !== source) return;
      if (!rememberEventId(event)) return;
      try { onEvent(JSON.parse(event.data)); } catch (_) { /* Ignore malformed events. */ }
    });
    source.addEventListener("error", () => void handleConnectionError(source));
  }

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") connect();
  });
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) connect();
  });
  connect();
})();
