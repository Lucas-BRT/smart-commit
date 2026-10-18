# npu-commit

Gera mensagens de commit no estilo do seu repositório usando um LLM pequeno
rodando **localmente na NPU Intel** (OpenVINO). Nada sai da sua máquina.

> Status: fases 1 a 3 concluídas. O fluxo completo funciona com um backend
> simulado (`--backend mock`); o backend da NPU é a fase 4.

```bash
cargo run -- --backend mock --mock-response 'feat: ...'   # gera e imprime a mensagem
cargo run -- --commit --backend mock --mock-response '…'  # abre o editor e commita
cargo run -- --backend mock --mock-response '…' --trace t.json
cargo run -- replay t.json    # refaz a geração gravada e confere se é idêntica
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
