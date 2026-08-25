const instanceId = "sidecar_dev_fixture_01";
let sequence = 0;

function event(name, payload) {
  sequence += 1;
  process.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: instanceId,
    streamId: "system",
    seq: sequence,
    event: name,
    payload,
  })}\n`);
}

event("system.ready", {
  protocolVersion: 1,
  sidecarSchemaVersion: 1,
  appSchemaVersion: 1,
  pid: process.pid,
  platform: process.platform,
  capabilities: ["system.status", "app.prepareQuit"],
  sidecarInstanceId: instanceId,
});

let buffered = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffered += chunk;
  let newline = buffered.indexOf("\n");
  while (newline !== -1) {
    const line = buffered.slice(0, newline);
    buffered = buffered.slice(newline + 1);
    try {
      const request = JSON.parse(line);
      if (request.method === "system.status") {
        respond(request.requestId, {
          state: "ready",
          sidecar: {
            protocolVersion: 1,
            sidecarSchemaVersion: 1,
            appSchemaVersion: 1,
            pid: process.pid,
            platform: process.platform,
            capabilities: ["system.status", "app.prepareQuit"],
            sidecarInstanceId: instanceId,
          },
          database: { health: "healthy", schemaVersion: 1 },
          recoveryRequired: false,
          blockerIds: [],
        });
      } else if (request.method === "app.prepareQuit") {
        respond(request.requestId, { safeToQuit: true, blockers: [] });
        process.stdin.pause();
      }
    } catch (error) {
      process.stderr.write(`fixture protocol error: ${String(error)}\n`);
      process.exitCode = 70;
    }
    newline = buffered.indexOf("\n");
  }
});

function respond(requestId, result) {
  process.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "response",
    sidecarInstanceId: instanceId,
    requestId,
    ok: true,
    result,
  })}\n`);
}
