# Yena

Yena is a local-first memory control plane for agentic systems.

## What This Repo Contains

- Product vision: `MASTER_VISION.md`
- Build roadmap: `PROJECT_PLAN.md`
- Work log and decisions: `AGENTS.md`
- Architecture details: `docs/ARCHITECTURE.md`
- Security assumptions: `docs/THREAT_MODEL.md`
- DB schema migrations: `db/migrations/`

## Phase 0 Status

- [x] Master vision and roadmap documented
- [x] Git initialized and connected to GitHub
- [x] Baseline architecture and threat model docs
- [x] Initial SQLite schema migration
- [x] Runtime scaffolding (Rust workspace)
- [ ] CI and tests

## Next Commands

1. Install Rust toolchain:

```bash
curl https://sh.rustup.rs -sSf | sh
```

2. Install SQL migration tool (choose one):

```bash
cargo install sqlx-cli --no-default-features --features sqlite
```

3. Build workspace:

```bash
cargo build
```

4. Start first implementation milestone:

```bash
# implement evidence append-only API in services/ingest-service
```
