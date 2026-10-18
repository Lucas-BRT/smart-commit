# 05 — Fase 5: hook `prepare-commit-msg`

> Status: concluída · Data: 2026-10-18 · Não usa a NPU (testada com o `MockBackend`)
>
> A fase 4 (backend da NPU) foi pulada por enquanto: esta máquina tem a NPU
> (Meteor Lake, `/dev/accel0`), mas ainda faltam o driver de espaço de
> usuário, o OpenVINO GenAI e o usuário no grupo `render`. O roadmap já
> previa que as fases sem NPU andassem em paralelo.

## O que entrega

A mensagem gerada aparece no editor de qualquer `git commit`, vindo do
terminal, do Zed, do lazygit ou de outro cliente:

```console
$ npu-commit hook install
installed /home/me/project/.git/hooks/prepare-commit-msg

$ git add . && git commit        # o editor abre com a mensagem já escrita

$ npu-commit hook uninstall
removed /home/me/project/.git/hooks/prepare-commit-msg
```

Enquanto o backend da NPU não existe, o hook usa o mock pelas variáveis de
ambiente (é assim que os testes funcionam):

```console
$ NPU_COMMIT_BACKEND=mock NPU_COMMIT_MOCK_RESPONSE='feat: ...' git commit
```

| Variável | Equivale a |
|---|---|
| `NPU_COMMIT_BACKEND` | `--backend` |
| `NPU_COMMIT_MOCK_RESPONSE` | `--mock-response` (uma resposta só) |
| `NPU_COMMIT_TRACE` | `--trace`: grava um trace de cada commit, para `replay` |

As flags têm precedência sobre as variáveis, que valem para todos os
comandos.

## Quando o hook age

O git passa ao hook o arquivo da mensagem e a origem dela. O hook só escreve
num **`git commit` simples**, que vai abrir o editor sem mensagem:

| Caso | Origem (`$2`) | Hook |
|---|---|---|
| `git commit`, `git commit -a`, `git commit <paths>` | vazia | **gera** |
| `-m`, `-F` | `message` | não mexe |
| `-t` / `commit.template` | `template` | não mexe |
| merge | `merge` | não mexe |
| `--squash` | `squash` | não mexe |
| `--amend`, `-c`, `-C` | `commit` | não mexe |

Mesmo num commit simples, nada é escrito se o arquivo já tiver uma mensagem
(linhas que não são comentário). Com `git commit -v`, o diff depois da linha
de tesoura não conta.

## Decisões

- **O hook nunca bloqueia o commit.** Backend indisponível, nada staged,
  erro do git: tudo vira `npu-commit: <erro>; the commit message was left as
  is` no stderr, e o hook sai com 0. O commit segue como se o hook não
  existisse.
- **O hook usa o índice que está sendo commitado.** O git roda o hook com
  `GIT_INDEX_FILE` apontando para o índice do commit, que é um índice
  temporário em `commit -a` e `commit <paths>`. O `git diff --staged` herda
  isso, então o modelo vê exatamente o que vai ser commitado (há teste
  cobrindo).
- **Caminho absoluto do binário no script**, entre aspas simples para o
  `sh`. Clientes gráficos costumam rodar hooks sem o `PATH` do shell do
  usuário. Se o binário mudar de lugar, basta rodar `hook install` de novo.
- **Hooks de terceiros são protegidos.** O script tem uma linha marcadora
  (`# npu-commit prepare-commit-msg hook`). Sem ela, `install` recusa,
  `--force` guarda o original em `prepare-commit-msg.npu-commit-backup`
  (sem nunca sobrescrever um backup anterior), e `uninstall` só remove o
  nosso e restaura o backup.
- **`core.hooksPath` é respeitado** (`git rev-parse --git-path hooks`), e
  **`core.commentChar` também**: com `auto`, assumimos `#`.
- **A mensagem entra antes dos comentários do git**, e o usuário edita e
  confirma como sempre.

## Reprodutibilidade

- `NPU_COMMIT_TRACE` permite gravar um trace de commits reais feitos por
  qualquer cliente, e esse trace se reproduz com `npu-commit replay` (há
  teste cobrindo). É o caminho para transformar uma mensagem ruim vista no
  dia a dia em fixture.
- Os testes commitam pelo git de verdade, isolados com
  `tests/common::isolate`. Ele aplica `GIT_ENV` (config, identidade e datas
  fixas) e remove as `NPU_COMMIT_*` do ambiente de quem roda os testes.
  Rodar `cargo test` com essas variáveis exportadas dá o mesmo resultado.
- O editor de teste é um `sh` portável, não `sed -i`, que só funciona com o
  sed do GNU.

## Testes

```bash
cargo test --locked   # 135 testes, nenhum precisa de NPU
```

- **Unitários** (87, sendo 8 novos): script e escape de aspas, marcador,
  quando agir, mensagem antes dos comentários, mensagem existente
  preservada, `commit -v`, `core.commentChar` personalizado.
- **Hook** (`tests/hook.rs`, 12): install (executável, caminho absoluto,
  reinstalação), commit simples, `-m`, `--amend`, merge, `commit -a` com
  índice temporário, falha do backend sem bloquear o commit, trace gravado
  pelo hook e reproduzido, proteção de hooks de terceiros com `--force`,
  backup e restauração, backup anterior preservado, uninstall sem hook,
  `core.hooksPath`.
- **CLI** (19, sendo 2 novos): opções vindas do ambiente, flags com
  precedência sobre ele.
- **Integração** (16, sendo 2 novos): `hooks_dir` com `core.hooksPath`
  relativo e absoluto; `comment_prefix`.
- **Fixtures** (1).

## Limitações conhecidas

- Até a fase 4, o hook instalado sem `NPU_COMMIT_BACKEND=mock` só mostra o
  aviso a cada commit.
- O hook gera a mensagem a cada commit, sem cache. Se a carga do modelo for
  lenta, o daemon da fase 7 resolve.
- No Windows o script `sh` depende do Git for Windows; não foi testado.
