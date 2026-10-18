# npu-commit

Gera mensagens de commit no estilo do seu repositório usando um LLM pequeno
rodando **localmente na NPU Intel** (OpenVINO). Nada sai da sua máquina.

> Status: fases 1 a 5 concluídas. O modelo gera mensagens na NPU (~6,6 s,
> usando 2,6× menos CPU) ou na CPU (~4 s), com resultado idêntico.

Setup (uma vez): `scripts/setup-npu.sh` (usa sudo), `scripts/check-npu.sh` e
`scripts/export-model.sh`. Veja [docs/06](docs/06-fase-4-npu.md).

```bash
cargo run                     # gera a mensagem na NPU (--device cpu|gpu)
cargo run -- --backend mock --mock-response 'feat: ...'   # sem modelo
cargo run -- --commit --backend mock --mock-response '…'  # abre o editor e commita
cargo run -- --backend mock --mock-response '…' --trace t.json
cargo run -- replay t.json    # refaz a geração gravada e confere se é idêntica
cargo run -- hook install     # preenche a mensagem de todo `git commit`
cargo run -- config init      # cria ~/.config/npu-commit/config.toml comentado
cargo run -- config set model.device cpu
cargo run -- config set --repo style.convention conventional   # .npu-commit.toml
cargo run -- config list --show-origin
cargo run -- context [PATH]   # mostra o que foi detectado no repositório
cargo run -- prompt [PATH]    # mostra o prompt que o modelo receberia
cargo test --locked           # não precisa de NPU
```

O toolchain é fixado em `rust-toolchain.toml`. Regras para contribuir
(testes, commits, reprodutibilidade) estão em [AGENTS.md](AGENTS.md).

## Documentação

- [01 — Design](docs/01-design.md)
- [02 — Fase 1: contexto do repositório](docs/02-fase-1-contexto.md)
- [03 — Fase 2: diff, prompt e saneamento](docs/03-fase-2-prompt.md)
- [04 — Fase 3: CLI de ponta a ponta e reprodutibilidade](docs/04-fase-3-cli.md)
- [05 — Fase 5: hook `prepare-commit-msg`](docs/05-fase-5-hook.md)
- [06 — Fase 4: backend OpenVINO GenAI na NPU](docs/06-fase-4-npu.md)
- [07 — Configuração global e por repositório](docs/07-configuracao.md)
