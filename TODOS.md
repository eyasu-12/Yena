# TODOS

## Core Memory Engine

### Import `AGENTS.md` / `CLAUDE.md` After Retrieval V2

**What:** Add a local import path that turns existing developer memory files into Yena evidence, memory proposals, observations, and benchmark cases.

**Why:** The first customer already maintains repo memory files. Yena should make adoption easy by ingesting the status quo instead of asking users to manually re-enter context.

**Context:** This should wait until retrieval v2 works well enough to show clear value. The intended product moment is `yena import AGENTS.md`, then asking Yena a project-memory question and receiving an answer with evidence and trace. Keep the first version narrow: local Markdown files only, no broad connector framework, no dashboard requirement.

**Effort:** M
**Priority:** P2
**Depends on:** Retrieval v2 working with developer ontology, observations, policy-filtered retrieval traces, and the developer-memory benchmark.

## Completed
