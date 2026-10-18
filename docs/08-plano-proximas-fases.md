# 08 — Plano: GPU, energia, qualidade e outros modelos

> Status: **proposta**, para discutir · Data: 2026-10-18
> Medições feitas com `scripts/experiments/` (exploratório) e com os
> testes de hardware, sempre nos prompts das fixtures.

## 1. O que aprendemos

### 1.1 Os três devices, mesmo modelo (Qwen2.5-Coder-1.5B INT4 sym)

Ponta a ponta pela CLI (release, cache cheio, 1 arquivo staged, média de 3):

| Device | Tempo total | CPU usada | Memória | TTFT | ms/token | Respostas |
|---|---|---|---|---|---|---|
| **GPU** (Arc iGPU, Arrow Lake-P) | **1,8 s** | **2,1 s** | **0,49 GB** | 60–360 ms | 35 | diferem da CPU em 2 de 5 |
| CPU (Core Ultra 5 225H) | 4,0 s | 11,1 s | 2,0 GB | 100–540 ms | 30 | referência |
| NPU (padrão atual) | 6,6 s | 4,2 s | 2,0 GB | ~2,4 s fixo | 78 | idênticas às da CPU |
| NPU com os ajustes de 1.2 | ~5 s (estimado) | — | — | **1,07 s** | **74** | idênticas |

- **A GPU ganha em tudo que medimos.** Ela precisa do compute runtime da
  Intel (`intel-opencl-icd`, `libigc2`…), que ainda não instalamos no
  sistema. Os testes rodaram com os `.deb` extraídos localmente.
- **A GPU não reproduz a CPU/NPU bit a bit.** O padrão dela é f16. Forçar
  f32 (`INFERENCE_PRECISION_HINT` + `KV_CACHE_PRECISION`) deixou tudo 12
  vezes mais lento e **ainda** diferente. Igualdade entre devices não é
  viável; o contrato de reprodutibilidade tem que ser **por device**. A
  GPU é determinística consigo mesma.
- **Energia não foi medida**, porque o RAPL exige root. Ela decide entre
  GPU e NPU como padrão (seção 3, item C).

### 1.2 A NPU estava mal configurada

O prefill dinâmico da NPU processa o prompt em blocos de
`NPUW_LLM_PREFILL_CHUNK_SIZE` (padrão 1024), e **só é ativado quando
`MAX_PROMPT_LEN` é maior que o bloco**. Com os nossos 1024 tokens ele ficava
desligado, e todo prompt pagava o custo de 1024. Isso explica o TTFT
constante.

| Configuração (prompt de 1024) | TTFT | ms/token | Compilação a frio |
|---|---|---|---|
| padrão | 2,37 s | 82 | 2–12 s |
| bloco 512 | **1,05 s** (−55%) | 82 | 4,5 s |
| `GENERATE_HINT=BEST_PERF` | 2,39 s | **73** (−10%) | 14 s |
| bloco 512 + `BEST_PERF` | **1,07 s** | **74** | 2–14 s |
| bloco 256 ou 128 | **não compila** (`UnrollPermuteDMA`, driver 1.38) | | |

As respostas ficaram idênticas em todas as variantes que compilaram.

### 1.3 Modelos

As versões já convertidas da organização OpenVINO no Hugging Face são
feitas com ferramentas mais novas: Qwen3.5 e LFM2.5 usam optimum 2.4-dev e
transformers 5.2. **No nosso runtime 2026.3.1 elas não funcionam**: o
LFM2.5 deu segfault e o Qwen3.5 é multimodal (precisa do `VLMPipeline`).
Para trocar de modelo de forma reproduzível, a exportação fica com a
gente, com a toolchain fixada (já é o que o `export-model.sh` faz).

Candidatos, todos ≤ 5B (com 15 GiB de RAM e INT4, cabe folgado):

| Modelo | Licença | NPU no 2026.3? | Prós | Contras |
|---|---|---|---|---|
| **Qwen2.5-Coder-1.5B-Instruct** (atual) | Apache-2.0 | sim (validado aqui) | rápido, focado em código, reproduzível | copia exemplos quando o diff diz pouco |
| **Qwen3-1.7B** | Apache-2.0 | listado desde 2025.3, mas há um bug conhecido ("Qwen3 falha ao carregar na NPU") | multilíngue (119 idiomas), mais recente | modo de raciocínio: exige `enable_thinking=false` |
| **Qwen3-4B-Instruct-2507** | Apache-2.0 | família Qwen3 (mesmo bug) | sem raciocínio, o melhor "faz-tudo" de 4B nas comparações | ~2,5× mais lento por token |
| **LFM2.5-1.2B-Instruct** | LFM 1.0 (limite de receita para uso comercial) | **sim, novo no 2026.3** (exportar com `--backup-precision int8_sym`) | arquitetura híbrida feita para edge, o menor | licença; qualidade em código não documentada |
| **SmolLM3-3B** | Apache-2.0 | **sim, novo no 2026.3** | português entre os 6 idiomas oficiais, totalmente aberto | fraco em código nos números publicados |
| **Phi-4-mini-instruct** (3,8B) | MIT | sem registro para NPU (o Phi-3.5 tem exemplo oficial) | forte em código e raciocínio na faixa de 4B | maior |
| **Gemma 4 E2B-it** | Apache-2.0 | suportado desde 2026.2 (devices não detalhados) | multilíngue (140+), recente | 5,1B no total (multimodal), 4,4 GB |
| Qwen3.5-0.8B / 2B | Apache-2.0 | **não** (só CPU/GPU) | recentes | multimodal (`VLMPipeline`), runtime mais novo |
| ~~Qwen2.5-Coder-3B~~ | licença de pesquisa | — | — | **descartado**: não comercial |
| ~~Llama 3.2 3B~~ | Llama 3.2 (gated) | — | — | **descartado**: licença e acesso |

**Não existe benchmark público de modelos pequenos para mensagens de
commit.** A literatura recente (CommitBench, CommitLLM) indica que o
alinhamento ao contexto do repositório pesa mais que o tamanho do modelo.
Ou seja: escolher o modelo pede uma **avaliação nossa** (item D), não
rankings genéricos.

## 2. Princípios para o que vem

1. **Medir antes de mudar o padrão.** Device e modelo padrão só mudam com
   números do `bench` (velocidade, CPU, energia) e do `eval` (qualidade).
2. **Reprodutível por device.** Todo resultado registra device, driver e
   runtime; as respostas de referência são guardadas por device.
3. **Cada item é uma sequência de commits com testes**, como no AGENTS.md.

## 3. Itens

### A. GPU como device de primeira classe (pequeno) — antes de tudo

- `scripts/setup-gpu.sh`: instala o compute runtime do **mesmo snapshot do
  PPA** que o driver da NPU validou (`intel-opencl-icd` 26.27, `libigc2`
  2.38.5, `libigdfcl2`, `libigdgmm12`), com SHA-256 fixados. A instalação
  pelo `dpkg` cria os links de soname: a falta deles produz um enganoso
  `CL_OUT_OF_HOST_MEMORY`.
- `scripts/check-npu.sh` vira `check-devices.sh`, com as checagens da GPU
  (`/dev/dri/renderD128` acessível, ICD da Intel registrado).
- O teste de hardware passa a rodar nos três devices.
- **Pronto quando:** `npu-commit --device gpu` funciona sem variáveis de
  ambiente e os testes de hardware passam na GPU.

### B. Ajustes da NPU (pequeno)

- Novas chaves em `config::KEYS`: `model.npu_prefill_chunk` (padrão 512) e
  `model.npu_generate_hint` (padrão `best_perf`). Elas vão para
  `pipeline_properties` e para a descrição do backend (logo, para o trace).
- `MAX_PROMPT_LEN` precisa ser múltiplo do bloco, e o bloco ≥ 512 (256 e
  128 não compilam no driver 1.38): validar na config, com mensagem clara.
- Teste de hardware: as respostas com e sem os ajustes são idênticas.
- **Pronto quando:** NPU com TTFT ≈ 1,1 s e ~5 s de ponta a ponta.

### C. Benchmark e energia (passo 2) (médio)

- `npu-commit bench [--device …] [--runs N]`: roda os prompts das fixtures
  e grava um JSON com TTFT, TPOT, tempo total, tempo de CPU, pico de
  memória e o ambiente (kernel, driver da NPU, runtime da GPU, OpenVINO,
  modelo pelo manifest).
- **Energia:** o RAPL do pacote (`intel-rapl:0`) cobre CPU, GPU e NPU do
  SoC, então é uma comparação justa entre os três. Ler o RAPL exige root.
  Opções: (a) uma regra udev que dá leitura ao grupo `render` só nesse
  arquivo, instalada pelo setup; (b) rodar só o `bench` com sudo. Medir com
  a máquina parada antes, para descontar o consumo em repouso.
- Resultado em `docs/benchmarks/<data>-<máquina>.json` + uma tabela no doc.
- **Pronto quando:** temos joules por mensagem nos três devices, e uma
  decisão de device padrão baseada nisso.

### D. Avaliação de qualidade (passo 3) (grande; o item mais importante)

- **Conjunto de dados versionado** (`tests/fixtures/eval/`): pares (diff,
  mensagem humana, histórico) de repositórios públicos com licença
  permissiva, fixados por SHA de commit, cobrindo Conventional, gitmoji e
  livre, em pt e en, além de casos difíceis (só lockfile, só renomeação,
  diff enorme). Só entram commits cuja mensagem humana é boa.
- **`npu-commit eval`**, com métricas determinísticas primeiro:
  - formato: segue a convenção pedida, assunto ≤ 72, idioma certo, ticket
    quando pedido;
  - taxa de nova tentativa e de fallback;
  - **cópia de exemplo**: a resposta é igual ou quase igual a um assunto do
    histórico (o defeito que já vimos);
  - menciona os arquivos ou símbolos certos do diff.
- **Semântica** (opcional, fase 2 do item): um modelo maior como juiz
  (por exemplo, Qwen3-8B na GPU), seguindo o protocolo do estudo de
  "LLM como avaliador" (cadeia de raciocínio + few-shot), sempre com o
  prompt e o modelo juiz fixados.
- Os relatórios são traces, então tudo pode ser reproduzido.
- **Pronto quando:** uma tabela modelo × device × métricas, rodável com um
  comando.

### E. Vários modelos (médio; depende de D)

- O `export-model.sh` passa a ler um registro
  (`scripts/export/models/<nome>.toml`, com modelo, revisão, flags e
  manifest esperado), mantendo a reprodutibilidade bit a bit e a toolchain
  fixada.
- Exportar e avaliar, nesta ordem: **Qwen3-1.7B** (`enable_thinking=false`
  via `ChatHistory::set_extra_context`), **LFM2.5-1.2B**, **SmolLM3-3B**,
  **Qwen3-4B-Instruct-2507**, **Phi-4-mini**.
- `npu-commit model list | fetch | use <nome>` (o `model fetch` estava no
  design), com a chave de config `model.name`.
- **Pronto quando:** pelo menos três modelos avaliados e um padrão
  escolhido pelos números de C e D.

### F. Device automático (passo 1) (pequeno; depende de A e C)

- `model.device = "auto"`, com a ordem em `model.device_order` (padrão
  decidido pelo item C; hoje os números apontam GPU → NPU → CPU).
- Ele tenta carregar na ordem e, se falhar, avisa e passa ao próximo. O
  **device efetivo** vai para a descrição do backend e para o trace. O
  `replay --with-model` explica que respostas diferentes entre devices são
  esperadas, em vez de dar só "differs".
- Falhar rápido: checar o device antes de compilar (`/dev/accel`,
  `/dev/dri`), para não pagar segundos de compilação a cada tentativa.
- **Pronto quando:** numa máquina sem NPU e sem GPU, o `auto` cai na CPU
  com um aviso, e os testes cobrem a ordem e o registro no trace (com um
  backend falso, sem hardware).

### G. Respostas de referência por device (pequeno)

- `tests/fixtures/golden/<modelo>/<device>.json`: as respostas esperadas
  para os prompts das fixtures. O teste de hardware compara com elas e
  aponta quando um driver ou runtime novo muda as respostas.
- Regenerar só com um script, como as fixtures de trace.

### H. Prompt guiado pela avaliação (médio; depende de D)

Só depois de ter métricas: instrução contra copiar exemplos, ordem das
seções, resumo do diff, mais exemplos ou menos. Cada mudança precisa
melhorar o `eval` sem piorar o formato.

### I. Depois

- Atualizar para OpenVINO 2026.4 quando houver um driver da NPU validado
  com ele (permite Qwen3.5 e as versões convertidas mais novas).
- Empacotamento (`cargo install`, `.deb`) e `npu-commit doctor`, que junta
  o `check-devices` e a config.
- Daemon: com o cache, a carga é de 1,7 s (NPU) ou ~1 s (GPU) por commit.
  Só se pagaria se o hook incomodar.

## 4. Ordem proposta

| # | Item | Tamanho | Por quê nesta ordem |
|---|---|---|---|
| 1 | **A** GPU | P | maior ganho medido e destrava C e F |
| 2 | **B** ajustes da NPU | P | −55% de TTFT com respostas idênticas, pronto para usar |
| 3 | **C** bench + energia (passo 2) | M | decide o device padrão |
| 4 | **F** device automático (passo 1) | P | usa a decisão de C |
| 5 | **G** referências por device | P | protege contra regressões antes de trocar o modelo |
| 6 | **D** avaliação (passo 3) | G | base para escolher o modelo e mexer no prompt |
| 7 | **E** outros modelos | M | depende de D |
| 8 | **H** prompt | M | depende de D |

## 5. Decisões em aberto

1. **Device padrão:** manter a NPU por identidade do projeto, ou `auto`
   com GPU primeiro se a energia confirmar? (Proposta: decidir no item C.)
2. **Instalar o runtime da GPU no sistema** (`setup-gpu.sh`, com sudo)?
3. **Leitura do RAPL:** regra udev para o grupo `render`, ou `bench` com
   sudo?
4. **Licença:** o uso comercial importa? Isso define se o LFM2.5 entra.
5. **Dados de avaliação:** quais repositórios públicos, e se os seus
   repositórios entram (os traces gravam diffs; ficariam locais, fora do
   git).

## Fontes

- [OpenVINO GenAI on NPU (2026)](https://docs.openvino.ai/2026/openvino-workflow-generative/inference-with-genai/inference-with-genai-on-npu.html): `MAX_PROMPT_LEN`, `NPUW_LLM_PREFILL_CHUNK_SIZE`, `PREFILL_HINT`, `GENERATE_HINT`, exportação
- [OpenVINO Release Notes (2026)](https://docs.openvino.ai/2026/about-openvino/release-notes-openvino.html): modelos por device, LFM2 na NPU, bug do Qwen3 na NPU
- [Edge AI and Vision — OpenVINO 2026.3](https://www.edge-ai-vision.com/2026/08/intel-releases-openvino-2026-3-with-expanded-generative-ai-and-model-support/)
- [Phoronix — OpenVINO 2026.0](https://www.phoronix.com/news/Intel-OpenVINO-2026.0-Released)
- [The Best Small Language Models in 2026](https://tinyweights.dev/posts/best-small-language-models-2026/) e [Intuz — Top 10 SLMs 2026](https://www.intuz.com/blog/best-small-language-models/) (blogs, números não verificados)
- [CodeIF (2025)](https://arxiv.org/pdf/2502.19166): instruction following em código
- [Evaluating Generated Commit Messages with LLMs](https://arxiv.org/pdf/2507.10906), [AI-Driven Code Documentation (CommitBench)](https://doi.org/10.3390/computers15020087), [CommitLLM](https://arxiv.org/html/2607.17532v1)
- Metadados de licença e tamanho: API do Hugging Face (`huggingface.co/api/models/...`), 2026-10-18
