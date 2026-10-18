import json, sys, time, glob
import openvino_genai as g
model, fixtures, cache = sys.argv[1:4]
prompts = [a["prompt"] for path in sorted(glob.glob(fixtures + "/*.json")) for a in json.load(open(path))["generation"]["attempts"]]
for name, props in {"GPU f32 + KV f32": {"INFERENCE_PRECISION_HINT": "f32", "KV_CACHE_PRECISION": "f32"}}.items():
    pipe = g.LLMPipeline(model, "GPU", CACHE_DIR=f"{cache}/{len(props)}", **props)
    cfg = g.GenerationConfig(max_new_tokens=128, do_sample=False)
    ttft, tpot, out = [], [], []
    for p in prompts:
        h = g.ChatHistory(); h.append({"role": "system", "content": p["system"]}); h.append({"role": "user", "content": p["user"]})
        r = pipe.generate(h, cfg); m = r.perf_metrics
        ttft.append(m.get_ttft().mean); tpot.append(m.get_tpot().mean); out.append(r.texts[0])
    print(f"{name:18} TTFT {sum(ttft[1:])/len(ttft[1:]):5.0f} ms  TPOT {sum(tpot)/len(tpot):5.1f} ms\n   {out}", flush=True)
