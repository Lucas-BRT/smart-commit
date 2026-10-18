# 02 — Fase 1: contexto do repositório e detecção de estilo

> Status: concluída · Data: 2026-10-18 · Não usa a NPU

## O que entrega

Tudo que o gerador vai precisar saber sobre o repositório **antes** de montar o
prompt. Para inspecionar à mão:

```console
$ smart-commit context ~/Projects/coffe-builder
repository:  /home/lucasbrt/Projects/coffe-builder
branch:      scenes
ticket:      -
convention:  Free (from 13 commits)
language:    Portuguese
types:       -
scopes:      -
examples:
  criação da cafeteria e outros fixes menores
  ...
staged (0):
```

## Módulos

| Módulo | Responsabilidade |
|---|---|
| `git` | `Repo`: encontra a raiz do repositório e roda o binário `git` para obter o histórico (`log --no-merges --format=%s`), a branch atual e os arquivos staged (`diff --staged --name-status -z`) |
| `style` | Funções puras sobre os assuntos dos commits: convenção, idioma, tipos e escopos, exemplos |
| `context` | `RepoContext::collect` junta tudo; `ticket_from_branch` |

## Decisões

- **Chamar o `git` em vez de usar uma lib (`gix`/`git2`).** Assim a config do
  usuário é respeitada e não entra nenhuma dependência nativa. Usamos as saídas
  "de máquina" (`-z`, `--format`), que são estáveis e funcionam com nomes de
  arquivo com espaços ou quebras de linha.
- **Histórico = últimos 30 commits sem merges** (`HISTORY_DEPTH`). Os merges
  têm assunto gerado pelo git e só poluiriam a amostra.
- **Convenção:** um estilo é adotado se **≥ 60%** dos assuntos o seguem
  (`CONVENTION_THRESHOLD`); senão o repositório é tratado como `Free`. Sem
  histórico, o resultado é `Unknown`, e a fase 2 vai usar um padrão
  configurável.
- **Idioma:** conta palavras comuns de pt e en **na descrição**, ou seja, depois
  de tirar o prefixo `feat(x):` ou o emoji. Sem isso, um `fix:` contaria como
  inglês. É preciso no mínimo 2 ocorrências; empate resulta em `Unknown`.
  Palavras ambíguas ("a", "no") ficam de fora.
- **Exemplos:** até 5 assuntos recentes, distintos, com 8–100 caracteres e que
  sigam a convenção detectada. São descartados como ruído: `fixup!`, `squash!`,
  `amend!`, `Revert "..."` e WIP (como palavra inteira, então "wipe" não é
  ruído).
- **Ticket:** só chaves no estilo Jira em maiúsculas (`ABC-123`) delimitadas na
  branch. Chaves em minúsculas foram deixadas de fora de propósito, porque
  `fix-2` daria falso positivo.

## Testes

```bash
cargo test            # 31 testes, nenhum precisa de NPU
```

- **Unitários** (22): parser do `--name-status -z` (renomeações, cópias,
  caminhos com `\n`, saída truncada), parser de Conventional Commits, gitmoji,
  convenção/idioma/escopos, filtro de ruído e exemplos, extração de ticket.
- **Integração** (`tests/git_context.rs`, 9): criam repositórios git
  temporários **isolados da config do desenvolvedor** (`GIT_CONFIG_GLOBAL=/dev/null`)
  e cobrem: diretório fora de repo, descoberta a partir de subdiretório,
  repositório vazio, staged antes do 1º commit, merges ignorados, HEAD
  destacado, ticket na branch, e add/modify/delete/rename staged (arquivos não
  staged não aparecem).

Validado também em 4 repositórios reais da máquina, com estilos livres em pt e
em en.

## Limitações conhecidas (para fases futuras)

- A detecção de idioma suporta só pt/en.
- ~~Ainda não detecta se o repositório costuma citar o ticket na mensagem.~~
  Resolvido na fase 2 (`TicketStyle`).
- Os assuntos são lidos, mas o corpo dos commits não (ex.: se o repositório usa
  corpo com bullets).
