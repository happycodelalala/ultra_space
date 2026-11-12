# Content

Game data that any implementation can load. Use **engine-neutral formats**: JSON (with JSON
Schema) or TOML. No engine-specific formats here.

| Folder | Contents |
|---|---|
| `schemas/` | JSON Schemas for every data type (parts, buildings, cultures, creatures, LLM outputs) |
| `data/` | Definitions: vehicle parts, buildings, items, tech tiers, culture kits, loot tables |
| `prompts/` | LLM prompt templates for the AI gateway (moddable) |
| `lore/` | World bible: hand-written lore and fixed facts generated content must respect |

Files in `data/<kind>s/` are validated against `schemas/<kind>.schema.json` by
`tools/check-content.py` (part of `tools/ci.sh`). Cross-field rules (parents exist, moons fit
inside their parent's sphere of influence, ...) are checked by the game's loaders.

| Data | Schema | Loader |
|---|---|---|
| `data/star_systems/*.json` | `schemas/star_system.schema.json` | `sim_core::system` |
