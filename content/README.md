# Content

Game data that any implementation can load. Use **engine-neutral formats**: JSON (with JSON
Schema) or TOML. No engine-specific formats here.

| Folder | Contents |
|---|---|
| `schemas/` | JSON Schemas for every data type (parts, buildings, cultures, creatures, LLM outputs) |
| `data/` | Definitions: vehicle parts, buildings, items, tech tiers, culture kits, loot tables |
| `prompts/` | LLM prompt templates for the AI gateway (moddable) |
| `lore/` | World bible: hand-written lore and fixed facts generated content must respect |

All data files are validated against their schemas by tooling in the implementation.
