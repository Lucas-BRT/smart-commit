# 03 — Fase 2: compactação do diff, prompt e saneamento

> Status: concluída · Data: 2026-10-18 · Não usa a NPU

## O que entrega

Tudo que fica entre o repositório e o modelo, nos dois sentidos: o diff
staged reduzido ao orçamento da NPU, o prompt montado a partir do estilo
detectado, e a limpeza determinística do que o modelo responder (mais um
fallback para quando a resposta não presta). Para inspecionar à mão:

```console
$ smart-commit prompt ~/Projects/coffe-builder
===== system =====
You write git commit messages from a diff. Reply with the commit message only: ...

Rules:
- Subject line of at most 72 characters, without a trailing period.
- Write in Brazilian Portuguese.
- Add a body only if the reason for the change is not obvious.

Recent subjects from this repository, follow their style:
criação da cafeteria e outros fixes menores
...
===== user =====
Files changed:
README.md | +17 -17
requirements.txt | +1 -0
src/camera.py | +1 -0

Diff:
--- README.md
@@ -1,17 +1,17 @@
...
--- requirements.txt
@@ -5 +5,2 @@ PyOpenGL==3.1.10
 PyOpenGL-accelerate==3.1.10
+pygame==2.6
...
===== ~473 of 1024 tokens =====
```

## Módulos

| Módulo | Responsabilidade |
|---|---|
| `git` | `Repo::staged_diff`: `diff --staged --numstat -z` + o patch com `-U1` |
| `diff` | `parse` junta os dois; `FileKind` classifica por caminho; `compact` encaixa tudo num orçamento de tokens |
| `prompt` | `build(ctx, files, options)` → `Prompt { system, user }`; `effective_style` aplica os padrões |
| `message` | `sanitize(raw, convention)` → `CommitMessage`; `fallback(changes, convention, language)` |
| `style` | ganhou `find_ticket` (antes em `context`) e `TicketStyle` |

## Decisões

### Diff

- **Patch e `--numstat` pareados por ordem.** O git lista os arquivos na mesma
  ordem nos dois comandos, então a n-ésima seção do patch é do n-ésimo arquivo.
  Assim não precisamos extrair caminhos dos cabeçalhos do patch, que podem vir
  entre aspas ou sem o prefixo `a/`/`b/` (`diff.noprefix`). Se as contagens não
  baterem, os hunks são descartados e só o stat sobra: o resultado fica pior,
  mas nunca errado.
- **`--no-ext-diff` e `--no-color`**: uma ferramenta de diff externa ou cor
  forçada na config do usuário quebraria o parse. Drivers `textconv`
  continuam valendo.
- **Classificação por caminho**, em ordem de prioridade: `Source` > `Config` >
  `Test` > `Docs`. `Lockfile`, `Generated` e `Binary` são ruído: aparecem no
  stat e nunca recebem orçamento para hunks.
- **Tokens estimados como `caracteres / 3`** (`CHARS_PER_TOKEN`), sem
  tokenizer. É pessimista de propósito: estourar o `MAX_PROMPT_LEN` da NPU faz
  a geração falhar, enquanto sobrar espaço só perde um pouco de contexto. A
  fase 4 deve medir isso com o tokenizer real.
- **Orçamento**: o stat vem primeiro e pode usar no máximo metade do
  orçamento; se não couber inteiro, o que sobrar vira `... and N more files`. O resto é dividido
  por camada de prioridade com *water-filling*: cada arquivo recebe uma parte
  igual do que sobrou, arquivos menores que a parte devolvem a sobra, e o que a
  camada não usar passa para a próxima.
- **Um arquivo só é cortado se receber ≥ 24 tokens** (`MIN_FILE_TOKENS`).
  Abaixo disso, poucas linhas de uma mudança grande confundem mais do que
  ajudam; o arquivo fica só no stat. Cortes são feitos em linhas inteiras, com
  `[... N more lines]`, e o prompt avisa quantos arquivos aparecem pela metade.

### Prompt

- **Instruções em inglês, mensagem no idioma do repositório.** Modelos
  pequenos seguem instruções em inglês com mais confiabilidade.
- **Orçamento do diff = `max_prompt_tokens` − instruções − 40 tokens de
  template de chat**, calculado já contando a nota de truncamento.
- Sem histórico (`Unknown`), valem os padrões de `PromptOptions`:
  Conventional Commits em inglês, com os tipos usuais listados. A config da
  fase 6 vai poder mudar isso.
- Tipos e escopos: no máximo 8 de cada, os mais usados.
- **Ticket**: só entra no prompt se a branch tem um e o repositório costuma
  citar tickets (`TicketStyle`): `Prefix` se a maioria dos assuntos começa
  com a chave, também logo depois do prefixo da convenção (`ABC-1: …`,
  `[ABC-1] …`, `feat: ABC-1 …`, `✨ ABC-1 …`), `Mention` se ela aparece em
  outro lugar. A instrução se adapta: "start the subject" no estilo livre,
  "start the description" com Conventional Commits ou gitmoji. O limiar é 50% dos assuntos (`TICKET_THRESHOLD`), menor que o da
  convenção porque chores e merges de main raramente têm ticket.

### Saneamento

Em ordem: remove blocos `<think>` (o Qwen3 emite; um bloco não fechado engole
o resto), pega o conteúdo do primeiro bloco ```` ``` ````, pula preâmbulos
("Here is the commit message:", "Mensagem de commit:"), tira rótulos
inline, marcadores de lista, `#`, aspas, crases e negrito do assunto, junta
espaços e remove o ponto final. Explicações depois da mensagem
("This commit message…", "**Explicação:**") são cortadas.

- **Assunto > 72 colunas** é cortado na última palavra que cabe, sem
  reticências.
- **Corpo** é rearrumado em 72 colunas, preservando parágrafos e bullets
  (`*` e `•` viram `-`).
- **Conventional Commits**: quase-acertos são corrigidos (`Feat: x`,
  `feat:x`, `feat : x`). Fora isso, o resultado é `NotConventional`, para o
  chamador tentar de novo. Gitmoji e estilo livre não são validados: errar ali
  custa pouco.
- **Fallback**: mensagem montada só a partir da lista de arquivos, no idioma
  e na convenção do repositório (`chore: atualiza 3 arquivos em src/`, com os
  arquivos no corpo). O tipo vira `docs` ou `test` se todos os arquivos forem
  desse tipo. A orquestração (tentar de novo uma vez e então usar o fallback)
  fica para a fase 3, junto do `Backend`.

## Testes

```bash
cargo test            # 82 testes, nenhum precisa de NPU
```

- **Unitários** (63, sendo 41 novos): parse do numstat (renomeações e
  binários), pareamento com o patch (inclusive `diff --cc` de conflito) e
  degradação quando não bate,
  classificação de caminhos, prioridade, redistribuição de sobras, orçamento
  respeitado em vários tamanhos, stat resumido; prompts em snapshot `insta`
  (`src/snapshots/`); saneamento de cerca de 20 formas de resposta ruim,
  `<think>`, explicações, correção de Conventional Commits, corte em 72
  colunas (também com acentos) e rewrap do corpo; fallback e `TicketStyle`.
- **Integração** (`tests/git_context.rs`, 13, sendo 4 novos): `staged_diff`
  num repositório real com modificação, arquivo com espaço, binário e
  renomeação; antes do primeiro commit; com `color.ui=always`,
  `diff.noprefix` e `diff.external` na config; e um prompt de 40 arquivos
  novos que precisa caber em 1024 tokens.
- **CLI** (`tests/cli.rs`, 6): o binário de ponta a ponta. Uso inválido sai
  com código 2, erros com código 1; `context` e `prompt` com e sem caminho,
  fora de repositório e sem nada staged. O `TestRepo` está em
  `tests/common/`.

Para atualizar os snapshots depois de mudar o prompt de propósito:
`INSTA_UPDATE=always cargo test` (ou `cargo insta review`) e revisar o diff.

## Limitações conhecidas (para fases futuras)

- A estimativa de tokens é heurística; falta calibrar com o tokenizer do
  modelo escolhido (fase 4).
- Mudanças só de espaço em branco gastam orçamento como qualquer outra. Usar
  `--ignore-space-change` esconderia commits que são só de formatação.
- Exemplos do histórico podem ter mais de 72 colunas (o limite da fase 1 é
  100) enquanto o prompt pede ≤ 72. Por enquanto o saneamento resolve.
- ~~Nos testes de integração, as chamadas que o próprio `Repo` faz ao git
  ainda leem a config global de quem roda os testes.~~ Resolvido com
  `Repo::with_env` e `GIT_ENV` em `tests/common/`, que também fixa
  identidade e datas dos commits de teste.
- O fallback de um único arquivo com caminho muito longo vira algo como
  `chore: add` depois do corte em 72 colunas; usar só o nome do arquivo
  nesse caso resolveria.
- O map-reduce para diffs grandes (design, §2.2, item 3) continua para a
  fase 7.
