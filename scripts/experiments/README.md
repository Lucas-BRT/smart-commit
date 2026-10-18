# Experimentos exploratórios (2026-10-18)

Scripts que geraram as medições de `docs/08-plano-proximas-fases.md`. São
exploratórios: rodam em Python, fora do `npu-commit`, só para decidir o que
implementar. Os números oficiais vão sair do `npu-commit bench` (item C do
plano).

Todos usam os prompts das fixtures (`tests/fixtures/traces`), então as
entradas são as mesmas em qualquer máquina.

## Ambiente

```bash
uv venv --python 3.11.17 /tmp/genai-venv
uv pip install --python /tmp/genai-venv/bin/python "openvino-genai==2026.3.1.0"
```

Para a GPU sem instalar o runtime no sistema: extraia os `.deb` de
`intel-opencl-icd`, `libigc2`, `libigdfcl2` e `libigdgmm12` (snapshot
`20260830T100000Z` do PPA `kobuk-team/intel-graphics`) num diretório `ROOT`,
rode `ldconfig -n ROOT/usr/lib/x86_64-linux-gnu` (sem os links de soname o
runtime falha com `CL_OUT_OF_HOST_MEMORY`), crie um `vendors/intel.icd`
apontando para `ROOT/usr/lib/x86_64-linux-gnu/intel-opencl/libigdrcl.so` e
exporte `OCL_ICD_VENDORS=vendors LD_LIBRARY_PATH=ROOT/usr/lib/x86_64-linux-gnu`.

## Scripts

| Script | Mede |
|---|---|
| `npu_hints.py MODEL FIXTURES CACHE` | NPU: `NPUW_LLM_PREFILL_CHUNK_SIZE` e `GENERATE_HINT`, com cache frio e quente |
| `gpu_precision.py MODEL FIXTURES CACHE` | GPU: f16 (padrão) contra f32 |
| `cand_gpu.py MODEL FIXTURES CACHE think\|nothink` | GPU: carga, TTFT, TPOT, memória e respostas de um modelo |
