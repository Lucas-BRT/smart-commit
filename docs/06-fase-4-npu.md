# 06 — Fase 4: backend OpenVINO GenAI na NPU

> Status: **em andamento** · Data: 2026-10-18
>
> O backend real funciona de ponta a ponta **na CPU**. Na NPU falta rodar o
> `scripts/setup-npu.sh` (precisa de `sudo`) e então os testes de hardware e
> o benchmark da seção 6.

## 1. O que entrega

```console
$ npu-commit                        # NPU, modelo padrão
$ npu-commit --device cpu           # ou gpu
$ npu-commit --model-dir DIR
$ npu-commit replay t.json --with-model --device cpu
```

Exemplo real (CPU, clone do `coffe-builder`, histórico livre em português):

```console
$ git diff --staged --stat
 src/camera.py | 1 +
$ npu-commit --device cpu --trace t.json
Atualiza a câmera para incluir uma função `speed()` que retorna 2.5
$ npu-commit replay t.json --with-model --device cpu
reproduced: 1 attempt(s), Model message
```

| Variável | Flag | Padrão |
|---|---|---|
| `NPU_COMMIT_DEVICE` | `--device npu\|gpu\|cpu` | `npu` |
| `NPU_COMMIT_MODEL_DIR` | `--model-dir` | `$XDG_DATA_HOME/npu-commit/models/qwen2.5-coder-1.5b-instruct-int4-sym-g128` |
| — | — | cache de compilação em `$XDG_CACHE_HOME/npu-commit/openvino` |

## 2. Setup da máquina (uma vez)

```bash
scripts/setup-npu.sh     # usa sudo; depois, logout/login
scripts/check-npu.sh     # confere tudo, sem mudar nada
scripts/export-model.sh  # sem sudo; ~2 min, ~10 GB temporários
```

O `setup-npu.sh` instala versões fixadas e confere o SHA-256 de cada
artefato:

| Componente | Versão | Por quê |
|---|---|---|
| `intel/linux-npu-driver` | 1.38.0 (Ubuntu 24.04) | última estável; Arrow Lake verificado |
| `libze1` (Level Zero) | 1.32.0, de um snapshot imutável do PPA | a validada com o driver |
| OpenVINO GenAI (archive) | 2026.3.1, em `/opt/intel/openvino_2026.3.1` | a validada com o driver |
| `libtbb12` | do apt | exigido pelo compilador da NPU |

Ele também cria o link `/opt/intel/openvino`, que é um dos caminhos onde os
crates `openvino-rs` procuram, e registra as bibliotecas em
`/etc/ld.so.conf.d`. As bibliotecas do archive não têm RUNPATH e contam com
o `setupvars.sh`, mas um hook rodado por um cliente git gráfico não passa
por shell nenhum. Sem esse registro, o `libopenvino_genai_c.so` era
encontrado mas não carregava (o teste de hardware achou isso). O script põe
o usuário no grupo `render`, dono de `/dev/accel/accel0`.

## 3. Modelo

**Qwen2.5-Coder-1.5B-Instruct**, exportado por nós. As variantes prontas da
organização OpenVINO no Hugging Face são `INT4_ASYM`, e a NPU exige pesos
**INT4 simétricos**. A única variante simétrica pequena é a de 8B, grande
demais para 15 GiB de RAM.

`scripts/export-model.sh` é reproduzível bit a bit. Duas exportações do
zero geraram arquivos idênticos, e o resultado esperado fica versionado em
`scripts/export/MANIFEST.sha256`; o script falha se a exportação sair
diferente. O que está fixado:

- o modelo, numa revisão do Hugging Face (`2e1fd39…`). Como o `optimum-cli`
  não aceita `--revision`, o script baixa essa revisão com `hf download` e
  exporta do diretório local;
- Python 3.11.17, via `uv`;
- os pacotes Python em `scripts/export/requirements.lock`, com hashes. As
  duas dependências que vêm do git (`optimum`, `optimum-onnx`) não têm hash,
  mas estão fixadas por commit. A combinação é a de
  `samples/export-requirements.txt` do próprio archive 2026.3.1
  (`transformers==5.0.0`, `optimum-intel` num commit, `nncf==3.3.0`), com o
  `openvino` igual ao runtime: o tokenizer exportado precisa casar com o
  `openvino-tokenizers` do archive;
- as flags: `--weight-format int4 --sym --group-size 128 --ratio 1.0`. O
  NNCF reportou 196/196 camadas em `int4_sym`, e só o embedding ficou em
  int8 (o padrão de backup).

O `MANIFEST.sha256` do modelo também é a identidade dele nos traces
(`manifest=a2130d0a95c994b2`).

## 4. Backend (`src/genai.rs`)

- **`runtime-linking`**: a biblioteca do GenAI é carregada quando o programa
  roda. O binário compila sem OpenVINO e sem libclang (o CI continua igual) e
  roda sem eles, dizendo como instalar.
- **Erros acionáveis antes de chamar o OpenVINO.** Os erros do binding não
  têm mensagem (`GeneralError`), então o modelo ausente, a biblioteca
  ausente e o cache são checados antes, cada um com o comando que resolve o
  problema.
- **Propriedades da NPU:** `MAX_PROMPT_LEN` igual ao orçamento do prompt
  (1024) e `MIN_RESPONSE_LEN` igual a `max_new_tokens` (128), as duas formas
  estáticas que a NPU compila. `CACHE_DIR` em todos os devices. As
  propriedades exclusivas da NPU não vão para CPU/GPU, que as rejeitariam.
- **Geração explícita:** `max_new_tokens`, `do_sample=false`, `num_beams=1`
  e `repetition_penalty=1.0` são sempre definidos, nunca herdados do
  `generation_config.json` do modelo.
- **Chat template do modelo** via `generate_with_history` (mensagens system e
  user).
- **`describe()`**, que vai para os traces:
  `openvino-genai device=NPU model=… manifest=… max_prompt_len=1024 min_response_len=128`.
  O `replay --with-model` avisa quando o backend atual difere do gravado.

## 5. Testes

```bash
cargo test --locked                                    # 151, sem hardware
cargo test --test hardware -- --ignored --test-threads 1   # modelo real
NPU_COMMIT_TEST_DEVICE=cpu cargo test --test hardware -- --ignored --test-threads 1
```

Os testes de hardware usam os prompts das fixtures de trace, então toda
execução vê as mesmas entradas:

- **gera uma resposta não vazia** para cada prompt, dentro de
  `max_new_tokens`;
- **o mesmo prompt dá a mesma resposta** (decodificação gulosa
  determinística);
- **a estimativa de tokens nunca fica abaixo da contagem real.** O orçamento
  depende disso: se a contagem real fosse maior, um prompt cheio estouraria
  o `MAX_PROMPT_LEN` da NPU.

Sem hardware: propriedades por device, identidade pelo manifest, descrição,
modelo ausente reportado antes de carregar o OpenVINO, diretórios XDG, e
CLI (`--model-dir`, `NPU_COMMIT_MODEL_DIR`, `--device` inválido,
`replay --with-model` sem modelo). Os testes isolam `XDG_DATA_HOME` e
`XDG_CACHE_HOME`, para nunca usar o modelo exportado na máquina de quem os
roda.

## 6. Medições

### CPU (Core Ultra 5 225H), prompts das fixtures

| Prompt | Tokens reais | Estimados | chars/token | TTFT | ms/token |
|---|---|---|---|---|---|
| conventional-pt-retry #1 | 207 | 245 | 3,54 | 2031 ms (1ª chamada) | 29,6 |
| conventional-pt-retry #2 | 248 | 311 | 3,75 | 184 ms | 29,1 |
| free-en-chatty #1 | 142 | 174 | 3,66 | 361 ms | 31,1 |
| gitmoji-fallback #1 | 201 | 226 | 3,36 | 537 ms | 31,9 |
| gitmoji-fallback #2 | 221 | 264 | 3,58 | 104 ms | 30,0 |

Carga do modelo: 1,26 s. Uma geração completa pela CLI, contando a carga,
levou cerca de 4 s.

- **A estimativa de 3 caracteres/token é pessimista como deveria:** os reais
  ficaram entre 3,36 e 3,75, uma margem de 15 a 25% contra estourar o limite
  da NPU.
- **Qualidade:** em `gitmoji-fallback` (um `LOGIN.md` e um `Cargo.lock`), o
  modelo copiou o primeiro exemplo do histórico (`✨ add login page`). Um
  modelo de 1,5B tende a imitar os exemplos; vale acompanhar com mais
  fixtures antes de mexer no prompt.

### NPU

A preencher depois do `setup-npu.sh`: tempo da 1ª carga com o cache vazio e
com o cache cheio, TTFT, ms/token e a comparação com a CPU.

## 7. Pendências

- [ ] Rodar `scripts/setup-npu.sh` + `check-npu.sh` (precisa de `sudo` e
      logout/login).
- [ ] Testes de hardware e medições na NPU.
- [ ] Se a NPU recusar o modelo de 1,5B, tentar o Qwen2.5-Coder-0.5B (que
      está na lista oficial de suporte) ou ajustar `--group-size`.
