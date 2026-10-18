# AGENTS.md

Instruções para agentes (e pessoas) que trabalham neste repositório.

## Projeto

CLI em Rust que gera mensagens de commit no estilo do repositório usando um
LLM rodando localmente na NPU Intel. O design e o roadmap estão em
[`docs/01-design.md`](docs/01-design.md), e cada fase concluída tem seu
documento em `docs/`.

## Verificação

O CI roda exatamente isto, e tudo precisa passar antes de qualquer commit:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

## Testes são obrigatórios

**Toda implementação deve vir junto com testes, no mesmo commit.** Não existe
"testes depois".

- **Funcionalidade nova:** testes cobrindo o caso normal e as bordas (entrada
  vazia, limites, erros).
- **Correção de bug:** um teste de regressão que falha sem a correção e passa
  com ela.
- **Refatoração:** os testes existentes precisam continuar passando sem
  alteração. Se for preciso mudar um teste, explique o motivo no commit.

Onde cada teste fica:

| Tipo | Local |
|---|---|
| Unitário | `#[cfg(test)] mod tests` no próprio módulo em `src/` |
| Snapshot de prompt | `insta::assert_snapshot!`, com arquivos em `src/snapshots/`. Atualize com `INSTA_UPDATE=always cargo test` e revise o diff |
| Integração com git | `tests/git_context.rs`, usando o `TestRepo` de `tests/common/` (repositório temporário isolado da config do desenvolvedor) |
| CLI de ponta a ponta | `tests/cli.rs`, rodando o binário via `CARGO_BIN_EXE_npu-commit` |
| Scripts | `tests/scripts/test-*.sh`, contra sysfs, releases e binários falsos (sem rede, sudo ou hardware); `shellcheck` em todos |
| Hardware (modelo real) | `tests/hardware.rs`, `#[ignore]`: `cargo test --test hardware -- --ignored --test-threads 1` (`NPU_COMMIT_TEST_DEVICE=cpu` sem NPU). Use os prompts das fixtures, nunca entradas ad hoc |

`cargo test` nunca pode depender da NPU, de rede ou da config git de quem
roda os testes.

## Reprodutibilidade

Qualquer resultado (build, teste ou mensagem gerada) precisa poder ser
reproduzido por outra pessoa, em outra máquina.

- **Build:** o toolchain fica fixado em `rust-toolchain.toml`, o
  `Cargo.lock` é versionado e o CI usa `--locked`. Não atualize dependências
  junto com outras mudanças; isso tem um commit próprio.
- **Testes determinísticos:** sem rede, sem NPU, sem relógio, sem
  aleatoriedade, sem a config git de quem roda e sem a config, os modelos e
  as variáveis `NPU_COMMIT_*` da máquina (veja `tests/common::isolate`). Nada que
  vá para a saída pode depender da ordem de iteração de um `HashMap`.
- **Geração determinística:** decodificação gulosa e parâmetros de geração
  explícitos (`GenerationConfig`), nunca padrões implícitos da biblioteca. Se
  algo precisar de aleatoriedade, a semente também é explícita. Uma nova
  tentativa precisa mudar o prompt de forma determinística, e não depender de
  sorte.
- **Dados de entrada versionados:** fixtures de teste ficam em
  `tests/fixtures/` ou inline no teste, nunca em caminhos da máquina de quem
  escreveu.
- **Gerações gravadas:** `npu-commit --trace FILE` grava uma geração e
  `npu-commit replay FILE` a refaz. Um caso ruim encontrado no uso real vira
  fixture em `tests/fixtures/traces/`, que o `tests/replay_fixtures.rs`
  reproduz. Mudanças que alteram o pipeline de propósito regeneram as
  fixtures com `tests/fixtures/make-traces.sh`, no mesmo commit, e o diff das
  fixtures é revisado. Fixtures nunca são editadas à mão.

## Commits

### Um commit por objetivo

Cada commit tem um objetivo só (uma funcionalidade, uma correção, uma
refatoração, uma mudança de docs), e o projeto precisa compilar e passar nos
testes em todos os commits.

### Mesmo arquivo, objetivos diferentes: adicione só os trechos

Quando um arquivo tem mudanças com objetivos diferentes (por exemplo, uma
correção de bug e uma funcionalidade nova no mesmo `src/diff.rs`), **não
adicione o arquivo inteiro**. Adicione só os trechos (hunks) de cada objetivo,
um commit por vez:

```bash
git add -p src/diff.rs        # escolha os hunks: y = adiciona, n = pula, s = divide, e = edita
git diff --staged             # confira o que vai no commit
git diff                      # confira o que fica para o próximo
```

Sem terminal interativo (agentes, scripts), faça o mesmo com um patch
parcial:

```bash
git diff src/diff.rs > /tmp/parcial.patch
# edite o patch e mantenha só os hunks deste objetivo
git apply --cached /tmp/parcial.patch
git diff --staged
```

Antes de cada commit, confirme que **só o que está staged** compila e passa
nos testes:

```bash
git stash push --keep-index --include-untracked
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git stash pop
```

### Mensagens

Siga o estilo do próprio histórico: Conventional Commits em português
(`feat: ...`, `fix(diff): ...`, `docs: ...`, `test: ...`), com assunto de no
máximo 72 colunas. Use o corpo para explicar o porquê, quando ele não for óbvio.

## Releases

- **Para lançar, suba `version` no `Cargo.toml`** (semver) num commit
  `chore: lança a versão X.Y.Z` na `main`. O `.github/workflows/release.yml`
  testa, empacota (`scripts/package.sh`, reproduzível) e publica a release,
  com notas geradas a partir dos commits (`scripts/release-notes.sh`). Nunca
  crie tags ou releases à mão.
- O tarball precisa continuar reproduzível: `tests/scripts/test-install.sh`
  empacota duas vezes e exige os mesmos bytes.
- **Mudou a exportação do modelo** (revisão, flags, lock)? Exporte duas vezes
  do zero, versione o novo `scripts/export/MANIFEST.sha256` e rode o workflow
  **Model**, que confere a reprodutibilidade em outra máquina e publica o
  modelo para o `fetch-model.sh`.
- Versões de drivers, runtimes, llama.cpp e modelos ficam fixadas com
  SHA-256 nos scripts; ao subir alguma, troque versão e hash juntos.

## Configuração

Toda configuração nova é declarada em `config::KEYS` (tipo, escopo,
variável de ambiente, descrição), nunca lida direto de um arquivo ou do
ambiente em outro lugar. Chaves sobre a máquina são `Scope::User` (só no
global). Se ela afeta a geração, precisa chegar ao trace por
`PromptOptions` ou `GenerationConfig`.

## Documentação

Ao concluir uma fase do roadmap, crie `docs/0N-fase-X-<tema>.md` no formato
dos anteriores (o que entrega, módulos, decisões, testes, limitações) e
atualize o `README.md`.
