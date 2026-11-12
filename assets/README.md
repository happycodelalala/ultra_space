# Assets

Art and audio, kept engine-neutral.

| Folder | Contents |
|---|---|
| `source/` | Working files: Blender `.blend`, layered textures, raw audio |
| `export/` | Engine-neutral exports the game loads: glTF/GLB models, PNG/KTX2 textures, OGG audio |

Implementations load from `export/`. Large binary files should move to Git LFS once the
asset volume grows.
