# Design

Game design, ideas and plans. **Engine-agnostic:** describe what the game does and why, not
which engine API does it. Engine-specific notes belong in `impl/<implementation>/`.

| Document | Contents |
|---|---|
| [technical-overview.md](technical-overview.md) | Game systems and how they work: scale, planets, simulation tiers, vehicles, cultures, combat, capital ships, AI/LLM systems, object model, persistence and streaming |
| [development-plan.md](development-plan.md) | Engine decision and rationale, parallel simulation architecture, environment, LLM integration, development batches |

Suggested additions as the design grows:

- `ideas/`: loose ideas and inspiration that aren't committed to yet
- `systems/`: one document per system when a section of the overview gets too large
- `decisions/`: short records of major decisions (what was decided, why, alternatives)
