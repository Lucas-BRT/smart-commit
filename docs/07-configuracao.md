# 07 — Configuração global e por repositório

> Status: concluída · Data: 2026-10-18 · Parte da fase 6 do roadmap

## O que entrega

```console
$ smart-commit config init                       # ~/.config/smart-commit/config.toml, tudo comentado
$ smart-commit config set model.device cpu       # global
$ smart-commit config set --repo style.convention conventional   # .smart-commit.toml, vai pro git
$ smart-commit config get style.language
$ smart-commit config list --show-origin
backend = openvino  (default)
model.device = cpu  (global /home/me/.config/smart-commit/config.toml)
style.convention = conventional  (repository /home/me/proj/.smart-commit.toml)
...
$ smart-commit config unset model.device
$ smart-commit config path [--repo]
```

## Camadas

Da mais forte para a mais fraca:

1. **flags** (`--backend`, `--device`, `--model-dir`);
2. **ambiente** (`SMART_COMMIT_BACKEND`, `SMART_COMMIT_DEVICE`,
   `SMART_COMMIT_MODEL_DIR`, `SMART_COMMIT_CACHE_DIR`);
3. **repositório**: `.smart-commit.toml` na raiz da work tree, versionado
   junto com o código;
4. **global**: `$XDG_CONFIG_HOME/smart-commit/config.toml` (padrão
   `~/.config/...`);
5. **padrões**.

`list --show-origin` mostra de qual camada vem cada valor. `set` e `unset`
avisam quando uma camada mais forte continua decidindo o valor (por
exemplo, mudar o global enquanto o repositório ou uma variável de ambiente
define a mesma chave).

## Chaves

| Chave | Valores | Padrão | Onde |
|---|---|---|---|
| `backend` | `openvino`, `mock` | `openvino` | só global |
| `model.device` | `npu`, `gpu`, `cpu` | `npu` | só global |
| `model.dir` | caminho absoluto ou `~/…` | `$XDG_DATA_HOME/smart-commit/models/<padrão>` | só global |
| `model.cache_dir` | caminho | `$XDG_CACHE_HOME/smart-commit/openvino` | só global |
| `model.max_prompt_tokens` | 256–8192 | 1024 | só global |
| `model.max_new_tokens` | 16–1024 | 128 | só global |
| `style.convention` | `auto`, `conventional`, `gitmoji`, `free` | `auto` | global ou repositório |
| `style.language` | `auto`, `english`, `portuguese` | `auto` | global ou repositório |
| `style.default_convention` | `conventional`, `gitmoji`, `free` | `conventional` | global ou repositório |
| `style.default_language` | `english`, `portuguese` | `english` | global ou repositório |
| `style.history_depth` | 1–500 | 30 | global ou repositório |

`auto` segue o histórico. Os `default_*` valem quando o `auto` não tem
histórico para aprender, ou não consegue decidir o idioma.

## Decisões

- **Um registro único de chaves** (`config::KEYS`): tipo, valores válidos,
  escopo, variável de ambiente e descrição. Leitura de arquivo, ambiente,
  flags, `set`, `list` e o template do `init` usam todos a mesma validação,
  então não existe chave aceita num lugar e recusada em outro. Um teste
  garante que o template, descomentado, lista todas as chaves com os
  valores padrão.
- **Nada é ignorado em silêncio.** Chave desconhecida, valor fora da faixa,
  tipo TOML errado e erro de sintaxe dão erro com o arquivo e a chave. Um
  `model.devise` digitado errado não pode passar despercebido. No hook,
  o erro vira aviso, como os outros: um arquivo quebrado não bloqueia
  commits.
- **Chaves de máquina só no global.** `backend` e `model.*` são recusados
  no `.smart-commit.toml`, seja pelo `set --repo` ou editando o arquivo à
  mão. Clonar um repositório não pode mudar qual modelo roda na sua máquina
  nem fazer você carregar um modelo de um caminho escolhido por outra
  pessoa.
- **`set`/`unset` preservam comentários** (`toml_edit`). Num arquivo
  criado pelo `init`, o `set` ativa a linha comentada no lugar dela, logo
  abaixo da descrição.
- **Caminhos:** absolutos ou `~/…` nos arquivos, sem ambiguidade sobre a
  que pasta um caminho relativo se refere. Um `--model-dir` relativo na
  linha de comando é resolvido a partir do diretório atual, como se espera
  de uma flag.
- **Convenção forçada:** `style.convention` diferente de `auto` tem
  precedência sobre a detecção. Os exemplos do histórico que contradizem a
  convenção forçada saem do prompt, senão o modelo receberia duas
  instruções opostas.
- O `context` mostra quando a convenção ou o idioma estão forçados pela
  config.

## Reprodutibilidade

- As configurações que afetam a geração (`style.*`, `max_prompt_tokens`,
  `max_new_tokens`) vão para o trace dentro de `options` e `config`. O
  `replay` não depende dos arquivos de config de quem roda. Os campos novos
  só são gravados quando têm valor, então os traces antigos continuam lidos
  igual e as fixtures regeneradas ficaram idênticas.
- Os testes isolam `XDG_CONFIG_HOME` (além de `XDG_DATA_HOME` e
  `XDG_CACHE_HOME`) e removem todas as `SMART_COMMIT_*`. A suíte passa igual
  com essas variáveis exportadas e com uma config global real na máquina.
  Testes que escrevem config usam um diretório temporário próprio.

## Testes

```bash
cargo test --locked   # 175, sem hardware
```

- **Unitários** (`config`, 13; `prompt`, 3 novos): camadas e origem,
  chaves desconhecidas, valores inválidos (tipo, faixa, caminho relativo),
  erro de sintaxe com o arquivo, chaves de máquina recusadas no
  repositório, `~/`, variáveis de ambiente vazias ignoradas, sem `$HOME`,
  `set`/`unset` preservando comentários, `set` validando antes de escrever,
  linha do template ativada no lugar, template completo e igual aos
  padrões; convenção/idioma forçados e exemplos filtrados.
- **CLI** (`tests/config_cli.rs`, 12): `init` (uma vez, `--force`,
  template do repositório), `set`/`get`/`unset`/`path`/`list`, valores
  inválidos sem escrever nada, repositório sobrepondo o global (e o aviso),
  chaves de máquina recusadas no repositório, ambiente sobrepondo arquivos,
  `backend` da config chegando à geração, `model.dir` da config e
  `--model-dir` relativo, convenção forçada chegando ao prompt e ao trace
  (que reproduz fora do repositório), `history_depth` chegando ao
  `context`, arquivo quebrado.
- **Hook** (1 novo): config inválida avisa e não bloqueia o commit.

## Limitações

- Não há camada "pessoal por repositório" (por exemplo, em `.git/`). Hoje,
  quem quiser uma configuração só sua num repositório usa as variáveis de
  ambiente.
- O fallback de device (NPU → GPU → CPU) ainda não é automático: escolha
  com `model.device`.
