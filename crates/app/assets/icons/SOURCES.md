# Icon sources
- provider-claude.svg: Simple Icons `claude` (v16.35.0), https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/claude.svg. CC0-1.0 file license; the Claude mark is an Anthropic trademark. Changes: dropped role/title, added fill="currentColor".
- provider-openrouter.svg: Simple Icons `openrouter`, https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/openrouter.svg. CC0-1.0; OpenRouter trademark. Same changes.
- provider-opencode.svg: Simple Icons `opencode`, https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/opencode.svg. CC0-1.0; OpenCode trademark. Same changes.
- provider-pi.svg: original hand-drawn pi glyph, no source. No SVG logos shipped in the Pi npm package. Original work, free to use.
No scripts, event attributes, foreignObject, or hrefs in any file.
- ../icon-packs/iconoir: Iconoir 7.12.1 (`icons/regular`), MIT. Vendored by scripts/vendor-icon-packs.py, renamed to Lucide names via scripts/icon-packs.json.
- ../icon-packs/remix: Remix Icon 4.9.1 (`-line` icons), Remix Icon License v1.0 (not Apache-2.0 despite the npm metadata). Same script and renaming.
- ../icon-packs/solar: Solar Linear via `@iconify-json/solar` 1.2.13, CC-BY-4.0. Credit: Solar Icon Set authors (480 Design), https://www.figma.com/community/file/1166831539721848736. Same script and renaming.
- Pack icons with no honest equivalent are null in the mapping and fall back to Lucide. The script rejects any pack SVG that fails XML parsing, has a non-svg root, or contains script, foreignObject, on* attributes or href.
- Each pack folder carries its upstream license as `LICENSE`.
