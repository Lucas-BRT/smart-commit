# 09 — Devices automáticos, GPUs de qualquer fabricante, instalação e releases

> Status: concluída · Data: 2026-10-18 · Itens A, B, C (sem energia) e F do
> [plano](08-plano-proximas-fases.md), mais instalador e releases

## O que entrega

```console
$ curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
...
==> Benchmark
Benchmark on Intel(R) Core(TM) Ultra 5 225H: 5 prompts, after one warm-up load.

  device   per message     load  generate     CPU
  gpu            1.0 s    0.6 s     0.4 s    1.1x  <- selected
  npu            3.4 s    1.8 s     1.6 s    0.4x
  vulkan         1.6 s    0.8 s     0.8 s    0.5x
  cpu            2.1 s    1.3 s     0.9 s    2.9x

smart-commit will use gpu on this machine (~1.0 s per message): the fastest, model.device = auto.
To use another device: smart-commit device use <npu|gpu|cpu|vulkan>, or `auto` for the fastest.

$ smart-commit device use npu
model.device = npu  (global /home/me/.config/smart-commit/config.toml)
~3.4 s per message on this machine, in the last benchmark.
```

## Devices e backends

| Device | Backend | Hardware |
|---|---|---|
| `npu` | OpenVINO GenAI | NPU Intel |
| `gpu` | OpenVINO GenAI | GPU Intel |
| `cpu` | OpenVINO GenAI | qualquer x86_64 |
| `vulkan` | **llama.cpp** (`llama-server`, Vulkan) | **qualquer GPU com Vulkan**: AMD (RADV), NVIDIA (driver proprietário ou NVK), Intel (ANV) |

**Por que um segundo backend:** o plugin de GPU do OpenVINO só funciona com
GPUs Intel (OpenCL/Level Zero da Intel). O llama.cpp com Vulkan cobre os
quatro casos pedidos, AMD e NVIDIA com driver proprietário ou aberto, com
**uma build só** (b11381, fixada por SHA-256). Para CUDA e ROCm o llama.cpp
também publica builds; ficam como otimização futura.

O backend llama.cpp (`src/llamacpp.rs`):

- sobe um `llama-server` privado a cada execução, ouvindo num **socket
  UNIX** dentro de uma pasta temporária `0700`. Nada fica exposto na rede.
  O caminho termina em `.sock` (é assim que o llama.cpp reconhece um
  socket) e fica abaixo do limite de 108 bytes;
- fala HTTP/1.1 por um cliente mínimo próprio, sem dependência de rede;
- faz requisições gulosas e com seed (`temperature 0`, `top_k 1`,
  `seed 0`, sem cache de prompt), com o contexto exatamente do tamanho do
  orçamento;
- usa o modelo **Qwen2.5-Coder-1.5B-Instruct Q4_K_M oficial da Qwen**
  (GGUF), fixado por revisão e SHA-256;
- grava no trace a build do servidor, o device Vulkan e o SHA-256 do GGUF.

Na iGPU Intel via Vulkan: o servidor fica pronto em 0,8 s, as respostas são
determinísticas, e ponta a ponta leva 2,5 s.

## `auto`, `bench` e `device`

- **`model.device = auto`** (o novo padrão) tenta os devices na ordem do
  último `bench` (o mais rápido primeiro) e depois na ordem padrão
  **gpu → npu → vulkan → cpu**. O primeiro que carrega vence, com um aviso
  dizendo quais foram pulados. Sem nenhum device, o erro lista o motivo de
  cada um.
- **`smart-commit bench`** mede os quatro devices nos **5 prompts das
  fixtures, embutidos no binário** (as mesmas entradas em qualquer máquina),
  depois de uma carga de aquecimento: carga, geração, custo por mensagem e
  núcleos de CPU usados. O relatório vai para
  `~/.local/share/smart-commit/bench.json`, com CPU, kernel, versão e a
  descrição de cada backend. Os critérios são o tempo e a CPU; a energia
  ainda não entra (o RAPL exige root).
- **`smart-commit device`** mostra a tabela, o device escolhido e de onde vem a
  escolha. **`device use <x>`** grava a config global e mostra o tempo
  estimado (ou avisa se aquele device falhou no último benchmark).

## Ajustes da NPU

`NPUW_LLM_PREFILL_CHUNK_SIZE=512` (`model.npu_prefill_chunk`) e
`GENERATE_HINT=BEST_PERF` (`model.npu_generate_hint`) agora são o padrão. Na
NPU, a mensagem caiu de **6,6 s para 5,1 s** de ponta a ponta, e um teste de
hardware confirma que as respostas são idênticas com e sem os ajustes. A
config exige que o bloco divida `model.max_prompt_tokens`. Blocos menores que
512 não compilam no driver 1.38.

## Scripts

| Script | O que faz | sudo |
|---|---|---|
| `install.sh` | baixa a release, confere o SHA-256, instala em `~/.local` e chama o setup; `--uninstall` desinstala | não |
| `scripts/setup.sh` | setup guiado: hardware, drivers (com confirmação), modelos e benchmark | só nos passos confirmados |
| `scripts/setup-npu.sh [--openvino-only]` | driver da NPU + OpenVINO, ou só o OpenVINO (para a CPU) | sim |
| `scripts/setup-gpu.sh [--dry-run] [--yes]` | GPUs Intel, AMD e NVIDIA, mais llama.cpp e GGUF | só para pacotes do sistema |
| `scripts/fetch-model.sh` | modelo do OpenVINO publicado pelo CI, conferido arquivo a arquivo | não |

`setup-gpu.sh`, por fabricante e driver do kernel (lido do sysfs):

| GPU | Driver | Instala |
|---|---|---|
| Intel | `i915`/`xe` | compute runtime da Intel para o OpenVINO (fixado no mesmo snapshot do PPA validado com o driver da NPU) + Mesa ANV |
| AMD | `amdgpu` | Mesa RADV (`radeon` antigo: aviso, o RADV exige `amdgpu`) |
| NVIDIA | `nvidia` (fechado ou módulos abertos) | o ICD Vulkan do próprio driver; se faltar, `libnvidia-gl-<versão em uso>`. **Nunca troca o driver** |
| NVIDIA | `nouveau` | Mesa NVK (aviso: Turing ou mais nova) |
| NVIDIA | nenhum | aviso: instale um driver (`ubuntu-drivers install`) ou use o nouveau |

Pacotes já instalados são pulados, e com `--dry-run` nada é executado.
Sem `apt`, o script lista os pacotes e faz só a parte do usuário.

## Releases e reprodutibilidade

- **Releases automáticas** (`.github/workflows/release.yml`): a cada push na
  `main`, se a versão do `Cargo.toml` ainda não tem tag, o workflow testa,
  empacota e publica a release, com notas geradas a partir dos commits
  (`scripts/release-notes.sh`, agrupadas por tipo). Para lançar uma versão,
  basta subir a versão. A build é feita no Ubuntu 22.04, então o binário
  roda no 22.04 e mais novos.
- **Tarball reproduzível** (`scripts/package.sh`): toolchain fixado,
  `--locked`, binário sem caminhos da máquina (`--remap-path-prefix`,
  `strip`), tar com ordem, dono e datas fixos (a data é a do commit) e gzip
  sem timestamp. Empacotar duas vezes dá o mesmo SHA-256 (testado).
- **Modelo publicado pelo CI** (`.github/workflows/model.yml`): o CI roda o
  `export-model.sh`, que falha se o resultado não for idêntico ao
  `MANIFEST.sha256` versionado, e publica o modelo numa release nomeada pelo
  hash do manifest. O `fetch-model.sh` confere cada arquivo contra o
  manifest do repositório.
- **O CI pegou uma falha de reprodutibilidade.** A exportação era idêntica
  duas vezes na mesma máquina, mas não no CI. Os pesos batiam; o que mudava
  era (1) o caminho absoluto da origem, gravado no `openvino_config.json`, e
  (2) os nomes que o OpenVINO dá aos nós que cria, que vêm de um contador
  global do processo (`Convert_110687` aqui, `Convert_110688` lá). A
  exportação agora usa um caminho relativo, e `normalize_ir.py` renumera
  esses nomes em ordem de aparição. O XML do CI, normalizado, ficou idêntico
  ao local, e as respostas não mudaram. O modelo passou a se chamar
  `e1c62f35ed8534a1`.

## Testes

```bash
cargo test --locked                       # 205, sem hardware
tests/scripts/test-setup-gpu.sh           # 9 cenários de GPU, com sysfs falso
tests/scripts/test-fetch-model.sh         # modelo bom, já instalado, adulterado
tests/scripts/test-setup.sh               # setup guiado, com release falsa
tests/scripts/test-normalize-ir.sh        # contadores diferentes, mesmo arquivo
tests/scripts/test-install.sh             # pacote reproduzível + instalação + desinstalação
tests/scripts/test-release-notes.sh       # notas agrupadas por tipo
cargo test --test hardware -- --ignored --test-threads 1   # SMART_COMMIT_TEST_DEVICE=npu|gpu|cpu|vulkan
```

- **Sem hardware:** seleção de device (ordem, backend fixo, device fixo,
  primeiro que carrega, todas as falhas), `bench` (ranking, tabela, ida e
  volta em JSON, aquecimento, falhas, tempo de CPU), llama.cpp (argumentos,
  requisição, parse de HTTP, servidor falso via socket, binário que morre ao
  carregar, arquivos ausentes), CLI de `bench`/`device` (com relatórios
  escritos à mão, incluindo o `auto` seguindo a ordem do benchmark),
  validação do bloco da NPU.
- **Hardware:** passa em `npu`, `cpu`, `vulkan` (iGPU Intel via Mesa ANV) e
  `gpu` (runtime extraído localmente). O teste novo confirma que os ajustes
  da NPU não mudam respostas.
- **AMD e NVIDIA** foram testados só pelos cenários do `setup-gpu.sh`: não
  há essas GPUs nesta máquina. O caminho Vulkan é o mesmo testado na Intel,
  mas o primeiro uso real numa AMD ou NVIDIA deve ser conferido com
  `smart-commit bench`.

## Limitações

- Não medimos energia: o RAPL exige root. O `auto` escolhe pelo tempo.
- Só Linux x86_64 e `apt`. Em outras distribuições, o setup lista os
  pacotes.
- O benchmark é por máquina. Depois de trocar driver, GPU ou modelo, rode
  `smart-commit bench` de novo.
- Em máquinas com várias GPUs Vulkan, usa-se a `model.vulkan_device`
  (padrão 0), e o `bench` mede só ela.
