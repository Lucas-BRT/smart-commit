import json, sys, time, glob, os, resource
import openvino_genai as g
model, fixtures, cache, think = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == "think"
prompts = [a["prompt"] for p in sorted(glob.glob(fixtures + "/*.json")) for a in json.load(open(p))["generation"]["attempts"]]
t = time.time()
pipe = g.LLMPipeline(model, "GPU", CACHE_DIR=cache)
load = time.time() - t
cfg = g.GenerationConfig(max_new_tokens=128, do_sample=False)
ttft, tpot, total, out = [], [], [], []
for p in prompts:
    h = g.ChatHistory(); h.append({"role": "system", "content": p["system"]}); h.append({"role": "user", "content": p["user"]})
    if not think: h.set_extra_context({"enable_thinking": False})
    t = time.time(); r = pipe.generate(h, cfg); total.append(time.time() - t); m = r.perf_metrics
    ttft.append(m.get_ttft().mean); tpot.append(m.get_tpot().mean); out.append(r.texts[0].strip()[:90])
rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1e6
name = os.path.basename(model) + (" (think)" if think else "")
print(f"{name:40} load {load:4.1f}s  TTFT {sum(ttft)/len(ttft):5.0f}ms  TPOT {sum(tpot)/len(tpot):5.1f}ms  gen/prompt {sum(total)/len(total):4.2f}s  RSS {rss:4.2f}GB")
for o in out: print("    ", repr(o))
