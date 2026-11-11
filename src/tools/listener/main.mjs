// `cadpole-listener`: the limited-privilege listener
// (docs/assistant-integration.md).
//
//   bazel run //src/tools/listener      # the running cadpole serve, over its socket
//
// It spawns `cadpole mcp --listener`, which reads the restricted token
// `cadpole serve` wrote for this run, and runs a model turn per request
// through the Claude Code installed on this machine, in headless mode, with
// the Cadpole tools that token grants and no others. This process never sees
// a token or a credential and never makes an HTTP request. `USAGE` below is
// the list of options.

import { McpError, McpStdioClient } from "./mcp_client.mjs";
import { Listener } from "./listener.mjs";
import { MIN_CLAUDE_VERSION, checkClaude, claudeDriver, echoDriver } from "./driver.mjs";
import { EXPECTED_DENIED, checkTools } from "./policy.mjs";

const USAGE = `usage: cadpole-listener [options]

Attach to a running \`cadpole serve\` as a limited-privilege listener. A
message typed in the UI (or a note on a selection) starts a turn; nothing
runs while nothing is typed.

Turns run with the installed Claude Code in headless mode (\`claude -p\`,
streaming JSON), given the Cadpole tools and nothing else; it must be
version ${MIN_CLAUDE_VERSION} or later. Headless mode: https://code.claude.com/docs/en/headless

  --server URL          the cadpole serve to attach to: unix:PATH or http://HOST:PORT
                        (CADPOLE_SERVER; default: the running one, over its socket)
  --cadpole PATH        the cadpole binary (CADPOLE_BIN, default "cadpole")
  --driver claude|echo  claude runs turns through the installed Claude Code;
                        echo runs the loop with no model (CADPOLE_LISTENER_DRIVER, default claude)
  --claude PATH         the Claude Code executable (CADPOLE_CLAUDE_BIN, default "claude")
  --model NAME          the model the turns run on (CADPOLE_LISTENER_MODEL,
                        default: the installed Claude Code's own default)
  --always-listen       act even when the UI's Listen toggle is off
  --max-turns N         model turns per message (CADPOLE_LISTENER_MAX_TURNS, default 12)
  --new-conversation-after N
                        start a fresh conversation after N messages
                        (CADPOLE_LISTENER_NEW_AFTER, default 30; 0 for never).
                        The message "/new" in the UI does the same at once.
  --poll-ms N           long-poll timeout (default 30000)
  --once                handle one poll and exit
  --client ID           the id the server keeps this listener's cursor under
                        (CADPOLE_LISTENER_CLIENT, default "listener")
  -h, --help            print this and exit
`;

const argv = process.argv.slice(2);
if (argv.includes("--help") || argv.includes("-h")) {
  process.stdout.write(USAGE);
  process.exit(0);
}
const flag = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const has = (name) => argv.includes(name);

const server = flag("--server") ?? process.env.CADPOLE_SERVER ?? "";
const cadpoleBin = flag("--cadpole") ?? process.env.CADPOLE_BIN ?? "cadpole";
const driverKind = flag("--driver") ?? process.env.CADPOLE_LISTENER_DRIVER ?? "claude";
const claudeBin = flag("--claude") ?? process.env.CADPOLE_CLAUDE_BIN ?? "claude";
const model = flag("--model") ?? process.env.CADPOLE_LISTENER_MODEL ?? "";
const newConversationAfter = Number(flag("--new-conversation-after") ?? process.env.CADPOLE_LISTENER_NEW_AFTER ?? 30);
const maxTurns = Number(flag("--max-turns") ?? process.env.CADPOLE_LISTENER_MAX_TURNS ?? 12);
const pollMs = Number(flag("--poll-ms") ?? 30_000);
const alwaysListen = has("--always-listen");
const once = has("--once");
// Stable across restarts on purpose: the server keeps the acked cursor under
// this string, and a restarted listener finds it only if it asks with the same
// one. Anything per-process (a pid) orphans the cursor and the
// restart silently starts at the tail.
const clientId = flag("--client") ?? process.env.CADPOLE_LISTENER_CLIENT ?? "listener";

const log = (s) => process.stderr.write(`cadpole-listener: ${s}\n`);

async function main() {
  if (driverKind !== "claude" && driverKind !== "echo") throw new Error(`--driver is claude or echo, not ${driverKind}`);
  // Before anything is attached: without a usable Claude Code there is
  // nothing to run a turn with.
  if (driverKind === "claude") log(`running turns through Claude Code ${checkClaude(claudeBin)} (${claudeBin})`);
  log(`attaching to ${server || "the running cadpole serve"} as a limited listener`);
  const cadpole = new McpStdioClient({
    command: cadpoleBin,
    args: ["mcp", ...(server ? ["--server", server] : []), "--listener"],
    onLog: (line) => log(line),
  });
  const info = await cadpole.start().catch(async (err) => {
    await cadpole.stop();
    // The binary itself could not be started: say how to point at one.
    if (err instanceof McpError && err.spawnFailed) throw new Error(`${err.message} (pass --cadpole PATH or set CADPOLE_BIN)`);
    throw err;
  });
  log(`connected to ${info?.serverInfo?.name ?? "cadpole"} ${info?.serverInfo?.version ?? ""}`.trim());

  // What the child publishes is what `capability::GRANTED` says, because the
  // child generates it from there. Checking it here turns a server that has
  // widened the listener's privileges into a visible warning instead of a
  // silent change.
  const tools = await cadpole.toolNames();
  const check = checkTools(tools);
  log(`tools: ${tools.length} (${tools.join(", ")})`);
  if (!check.ok) {
    log(`WARNING: the tool list is not what this listener expects.`);
    if (check.unexpected.length) log(`  the server offered: ${check.unexpected.join(", ")}`);
    if (check.missing.length) log(`  the server withheld: ${check.missing.join(", ")}`);
  }
  for (const denied of EXPECTED_DENIED) {
    if (tools.includes(denied)) log(`WARNING: ${denied} should not be available to a listener token`);
  }

  const call = (name, args) => cadpole.callTool(name, args);
  const driver =
    driverKind === "echo"
      ? echoDriver({ call })
      : claudeDriver({ server, cadpoleBin, claudeBin, tools, maxTurns, model, newConversationAfter, log });
  log(`driver: ${driverKind}`);

  const listener = new Listener({ cadpole, driver, log, alwaysListen, pollMs, client: clientId });
  const shutdown = async () => {
    log("stopping");
    await listener.stop();
    await cadpole.stop();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  await listener.resume();
  if (once) {
    await listener.step();
    await listener.stop();
    await cadpole.stop();
    return;
  }
  await listener.run();
}

main().catch((err) => {
  log(`fatal: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
});
