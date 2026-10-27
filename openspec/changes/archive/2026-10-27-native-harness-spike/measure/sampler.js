// Runs inside the container: spawns argv, samples the RSS of every other process every 250 ms,
// and writes /out/result.json with peak/median tree RSS (KiB) and the cgroup memory.peak (bytes).
const { spawn } = require("child_process");
const fs = require("fs");
const samples = [];
let peak = { kib: 0, procs: [] };
function sample() {
  let sum = 0, procs = [];
  for (const d of fs.readdirSync("/proc")) {
    if (!/^\d+$/.test(d) || +d === process.pid) continue;
    try {
      const st = fs.readFileSync(`/proc/${d}/status`, "utf8");
      const kb = +(/VmRSS:\s+(\d+)/.exec(st) || [0, 0])[1];
      sum += kb; procs.push(`${/Name:\s+(\S+)/.exec(st)[1]}:${kb}`);
    } catch {}
  }
  samples.push(sum);
  if (sum > peak.kib) peak = { kib: sum, procs };
}
const t = setInterval(sample, 250);
const t0 = Date.now();
const child = spawn(process.argv[2], process.argv.slice(3), { stdio: "inherit" });
child.on("exit", (code) => {
  clearInterval(t);
  const s = [...samples].sort((a, b) => a - b);
  const cg = +fs.readFileSync("/sys/fs/cgroup/memory.peak", "utf8");
  fs.writeFileSync("/out/result.json", JSON.stringify({
    exit: code, seconds: (Date.now() - t0) / 1000, samples: s.length,
    peak_rss_kib: peak.kib, peak_procs: peak.procs.sort(),
    steady_rss_kib: s[Math.floor(s.length / 2)] || 0, cgroup_memory_peak_bytes: cg,
  }));
  process.exit(0);
});
