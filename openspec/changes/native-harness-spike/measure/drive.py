#!/usr/bin/env python3
"""Minimal ACP client: runs one prompt against the agent command in argv, auto-approves
permissions, prints a JSON summary (stop reason, tool calls, wall time, last usage_update)."""
import json, subprocess, sys, time
prompt = open(sys.argv[1]).read()
cwd = sys.argv[2]
p = subprocess.Popen(sys.argv[3:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def send(o): p.stdin.write(json.dumps(o) + "\n"); p.stdin.flush()
nid = [0]
def req(m, params):
    nid[0] += 1; send({"jsonrpc": "2.0", "id": nid[0], "method": m, "params": params}); return nid[0]
stats = {"tool_calls": 0, "thought_chunks": 0, "message_chunks": 0, "usage": None}
def wait(i):
    while True:
        line = p.stdout.readline()
        if not line: raise SystemExit("agent closed stdout")
        try: m = json.loads(line)
        except ValueError: continue
        if m.get("method") == "session/request_permission":
            opts = m["params"]["options"]
            pick = next((o for o in opts if o["kind"].startswith("allow")), opts[0])
            send({"jsonrpc": "2.0", "id": m["id"], "result": {"outcome": {"outcome": "selected", "optionId": pick["optionId"]}}})
        elif m.get("method") == "session/update":
            u = m["params"]["update"]; k = u.get("sessionUpdate")
            if k == "tool_call": stats["tool_calls"] += 1
            elif k == "agent_thought_chunk": stats["thought_chunks"] += 1
            elif k == "agent_message_chunk": stats["message_chunks"] += 1
            elif k == "usage_update": stats["usage"] = {"used": u.get("used"), "size": u.get("size")}
        elif m.get("id") == i and "method" not in m:
            return m
t0 = time.time()
wait(req("initialize", {"protocolVersion": 1, "clientCapabilities": {}}))
sid = wait(req("session/new", {"cwd": cwd, "mcpServers": []}))["result"]["sessionId"]
res = wait(req("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": prompt}]}))
stats["stop_reason"] = res.get("result", {}).get("stopReason") or res.get("error")
stats["wall_seconds"] = round(time.time() - t0, 1)
p.stdin.close()
try: p.wait(timeout=10)
except subprocess.TimeoutExpired: p.kill()
print(json.dumps(stats))
