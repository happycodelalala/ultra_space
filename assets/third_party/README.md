# Third-party assets

Source pieces from CC0 packs (public domain; no attribution required), kept as downloaded so
the building kits can be rebuilt (`tools/build-kits.sh`). Each folder holds the pack's own
licence file. They are placeholders until the design track's own kits.

| Folder | Pack | Source | Licence |
|---|---|---|---|
| kenney-fantasy-town | Fantasy Town Kit 2.0 (GLB) | https://kenney.nl/assets/fantasy-town-kit | CC0 |
| kenney-space-kit | Space Kit (GLB) | https://kenney.nl/assets/space-kit | CC0 |
| kenney-castle-kit | Castle Kit 2.0 (GLB, the pieces used: walls, towers, gates) | https://kenney.nl/assets/castle-kit | CC0 |
| kenney-survival-kit | Survival Kit 2.0 (GLB, the pieces used) | https://kenney.nl/assets/survival-kit | CC0 |
| kenney-pirate-kit | Pirate Kit 2.1 (GLB, the pieces used) | https://kenney.nl/assets/pirate-kit | CC0 |
| kenney-city-industrial | City Kit Industrial 2.0 (GLB, the pieces used) | https://kenney.nl/assets/city-kit-industrial | CC0 |
| kenney-city-commercial | City Kit Commercial 2.1 (GLB, the pieces used) | https://kenney.nl/assets/city-kit-commercial | CC0 |
| kenney-city-suburban | City Kit Suburban 2.0 (GLB, the pieces used) | https://kenney.nl/assets/city-kit-suburban | CC0 |
| quaternius | Universal Animation Library, Universal Base Characters, Modular Character Outfits - Fantasy (Standard, free) | https://quaternius.com/packs/universalanimationlibrary.html, https://quaternius.com/packs/universalbasecharacters.html, https://quaternius.com/packs/modularcharacteroutfitsfantasy.html | CC0 |

The Quaternius character packs are too large to keep here (about 130 MB of 4K textures): only
their licence is. `tools/build-characters.sh` reads them from `$ASSET_LIBRARY` (default
`~/asset-library`, laid out as `quaternius-ual/`, `q-universalbasecharacters/` and
`q-modularcharacteroutfitsfantasy/`, each with the pack's zip extracted to `extracted/`) and
writes the built characters, textures reduced to 512 px, to `assets/export/characters/`.
