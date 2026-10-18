import json, sys, time, glob
import openvino_genai as g
model, fixtures, cache = sys.argv[1], sys.argv[2], sys.argv[3]
prompts = []
for path in sorted(glob.glob(fixtures + "/*.json")):
    for a in json.load(open(path))["generation"]["attempts"]:
        prompts.append(a["prompt"])
variants = {
    "chunk 512 + BEST_PERF": {"NPUW_LLM_PREFILL_CHUNK_SIZE": 512, "GENERATE_HINT": "BEST_PERF"},
}
for name, extra in variants.items():
    props = {"MAX_PROMPT_LEN": 1024, "MIN_RESPONSE_LEN": 128, "CACHE_DIR": f"{cache}/{len(extra)}-{abs(hash(name))}"}
    props.update(extra)
    for run in ("cold", "warm"):
        t = time.time()
        try:
            pipe = g.LLMPipeline(model, "NPU", **props)
        except Exception as e:
            print(f"{name:36} {run}: FAILED to compile: {str(e).splitlines()[-1][:120]}", flush=True)
            out = []
            break
        load = time.time() - t
        cfg = g.GenerationConfig(max_new_tokens=128, do_sample=False)
        ttft, tpot, out = [], [], []
        for p in prompts:
            h = g.ChatHistory()
            h.append({"role": "system", "content": p["system"]})
            h.append({"role": "user", "content": p["user"]})
            r = pipe.generate(h, cfg)
            m = r.perf_metrics
            ttft.append(m.get_ttft().mean); tpot.append(m.get_tpot().mean); out.append(r.texts[0])
        print(f"{name:36} {run}: load {load:5.1f}s  TTFT {sum(ttft)/len(ttft):6.0f} ms  TPOT {sum(tpot)/len(tpot):5.1f} ms", flush=True)
        del pipe
    print("   replies:", out, flush=True)
