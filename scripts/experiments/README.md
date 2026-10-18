# Exploratory experiments (2026-10-18)

The scripts behind the device and model measurements in
[docs/devices.md](../../docs/devices.md) and
[docs/roadmap.md](../../docs/roadmap.md). They are exploratory: Python,
outside smart-commit, written to decide what to build. Device comparisons for
users come from `smart-commit bench`.

They all run the prompts of the trace fixtures (`tests/fixtures/traces`), so
the inputs are the same on every machine.

## Environment

```bash
uv venv --python 3.11.17 /tmp/genai-venv
uv pip install --python /tmp/genai-venv/bin/python "openvino-genai==2026.3.1.0"
```

To try the Intel GPU without installing its runtime system-wide: extract the
`.deb` files of `intel-opencl-icd`, `libigc2`, `libigdfcl2` and `libigdgmm12`
(snapshot `20260830T100000Z` of the `kobuk-team/intel-graphics` PPA) into a
directory `ROOT`, run `ldconfig -n ROOT/usr/lib/x86_64-linux-gnu` (without
the soname links the runtime fails with `CL_OUT_OF_HOST_MEMORY`), write a
`vendors/intel.icd` that points at
`ROOT/usr/lib/x86_64-linux-gnu/intel-opencl/libigdrcl.so`, and export
`OCL_ICD_VENDORS=vendors LD_LIBRARY_PATH=ROOT/usr/lib/x86_64-linux-gnu`.

## Scripts

| Script | Measures |
|---|---|
| `npu_hints.py MODEL FIXTURES CACHE` | NPU: `NPUW_LLM_PREFILL_CHUNK_SIZE` and `GENERATE_HINT`, with cold and warm caches |
| `gpu_precision.py MODEL FIXTURES CACHE` | GPU: f16 (the default) against f32 |
| `cand_gpu.py MODEL FIXTURES CACHE think\|nothink` | GPU: load, time to first token, time per token, memory and replies of a model |
