# 04 — Fase 3: CLI de ponta a ponta e reprodutibilidade

> Status: concluída · Data: 2026-10-18 · Não usa a NPU (usa o `MockBackend`)

## O que entrega

O fluxo completo, do que está staged até o commit, com o modelo atrás de um
trait. Como o backend da NPU só chega na fase 4, por enquanto a CLI roda com
`--backend mock`, que responde com textos dados na linha de comando:

```console
$ npu-commit --backend mock --mock-response $'```\natualiza dependências e a câmera\n```'
atualiza dependências e a câmera

$ npu-commit --commit --backend mock --mock-response 'feat: ...'   # abre o editor e commita

$ npu-commit --backend mock --mock-response '...' --trace trace.json
$ npu-commit replay trace.json
reproduced: 1 attempt(s), Model message
atualiza dependências e a câmera

$ npu-commit
error: the NPU backend is not available yet; use --backend mock
```

| Comando | O que faz |
|---|---|
| `npu-commit [-C PATH]` | gera a mensagem para o que está staged e a imprime |
| `npu-commit --commit` | abre o editor com a mensagem (`git commit --edit`) |
| `npu-commit --trace FILE` | grava a geração para `replay` |
| `npu-commit replay FILE` | refaz a geração gravada e confere se é idêntica |
| `npu-commit context [PATH]`, `prompt [PATH]` | inspeção (fases 1 e 2) |

Uso inválido sai com código 2 (clap); erros, com código 1.

## Módulos

| Módulo | Responsabilidade |
|---|---|
| `backend` | trait `Backend`, `GenerationConfig`, `MockBackend` |
| `generate` | `generate()`: prompt → modelo → saneamento → nova tentativa → fallback; devolve todas as tentativas |
| `trace` | `Trace::new`, `to_json`/`from_json`, `replay`/`replay_with` |
| `context` | `RepoContext::new` constrói o contexto a partir dos dados brutos do git |
| `git` | `commit_in_editor`; `with_env` para isolar as chamadas ao git |

## Reprodutibilidade

O objetivo é que qualquer resultado possa ser refeito por outra pessoa, em
outra máquina:

- **Build:** toolchain fixado em `rust-toolchain.toml` (1.99.1),
  `rust-version` no `Cargo.toml`, `Cargo.lock` versionado e `--locked` no CI.
- **Geração:** `GenerationConfig` torna explícito tudo que afeta a saída do
  modelo (`max_new_tokens = 128`), e a decodificação é sempre gulosa. Por isso
  não há semente: nada é sorteado.
- **Nova tentativa determinística.** Com decodificação gulosa, repetir o mesmo
  prompt daria a mesma resposta. Por isso a segunda tentativa leva uma nota
  dizendo por que a primeira foi rejeitada ("Your previous reply was rejected
  because …"). O diff encolhe para dar espaço a ela
  (`prompt::build_with_note`).
- **Traces.** `--trace` grava o que o git reportou (branch, assuntos, arquivos
  staged, diff), as opções, o `GenerationConfig`, o backend e cada tentativa
  (prompt, resposta bruta, motivo da rejeição), além da mensagem final.
  `replay` refaz tudo com as respostas gravadas no lugar do modelo e aponta as
  diferenças. `Trace::replay_with` vai permitir, na fase 4, conferir se o
  modelo real ainda responde igual.
- **Fixtures.** `tests/fixtures/traces/` tem casos gerados por
  `tests/fixtures/make-traces.sh` em repositórios com identidade, datas e
  config fixas. Rodar o script duas vezes gera arquivos idênticos byte a byte.
  `tests/replay_fixtures.rs` exige que todos se reproduzam. Um caso ruim
  encontrado no uso real vira uma fixture nova.
- **Testes isolados:** `GIT_ENV` (em `tests/common/`) isola da config global
  do usuário tanto o `TestRepo` quanto o `Repo` (`with_env`) e o binário, e
  fixa autor e datas dos commits.

## Decisões

- **Erros do backend não caem no fallback.** Se a NPU falhar, o usuário
  precisa saber; o fallback é só para saídas inválidas do modelo.
- **No máximo 2 tentativas** (`MAX_ATTEMPTS`). Uma nova tentativa custa
  outra geração inteira na NPU.
- **`--commit` usa `git commit --edit --message`**: hooks, limpeza da
  mensagem e cancelamento com mensagem vazia continuam sendo do git. Se o
  editor falhar, nada é commitado.
- **O trace não guarda o caminho do repositório**, que é específico da
  máquina e pode expor o nome do usuário. Ele guarda o diff e os assuntos,
  por isso só é gravado quando pedido.
- **Formato versionado** (`format: 1`): um trace de formato desconhecido é
  recusado com uma mensagem clara, em vez de ser lido errado.

## Testes

```bash
cargo test --locked   # 111 testes, nenhum precisa de NPU
```

- **Unitários** (79): `MockBackend`; orquestração (primeira resposta válida,
  nova tentativa com a nota exata, fallback, erro do backend propagado,
  mesma entrada → mesma geração); nota no prompt cabendo no orçamento; trace
  (ida e volta em JSON sem o caminho, replay exato, diferenças em prompt,
  mensagem e entradas, formato desconhecido); `RepoContext::new`.
- **CLI** (17): geração com saneamento, nova tentativa, fallback com aviso,
  sem backend, nada staged, `-C`, `--commit` com `GIT_EDITOR=true` (commit
  criado com a mensagem) e `GIT_EDITOR=false` (nada commitado), `--trace` +
  `replay` fora do repositório, `replay` com trace adulterado e com arquivo
  inexistente.
- **Integração** (14): agora com `Repo::with_env`, testado mudando
  `diff.renames` por uma config global de teste.
- **Fixtures** (1): todos os traces de `tests/fixtures/traces/`.

## Limitações conhecidas (para fases futuras)

- O backend da NPU (fase 4) ainda não existe; sem `--backend mock`, a
  geração falha com uma mensagem clara.
- `PromptOptions` e `GenerationConfig` ainda usam só os valores padrão; a
  config em `~/.config/npu-commit/` é da fase 6. Os dois já entram no trace.
- O trace guarda a versão do npu-commit, mas não o hash do modelo; isso
  entra com o backend real.
