# UltraTasks development

- UI/UX quality is the primary acceptance criterion. Start from the sources and criteria in `docs/`, and reuse suitable OSS behavior before writing new primitives.
- The initial product is calendar + ToDo. Keep documents, kanban, AI and MCP outside the first implementation slice unless explicitly requested.
- Product clients must be native Rust/GPUI with no WebView. Do not silently substitute Electron, Tauri, React or a website. Android feasibility is an open spike; report evidence and blockers.
- Keep local data operations independent of network availability. Task, Event and TimeBlock have distinct semantics.
- Route user edits through shared application commands with validation and undo. Future AI/MCP uses the same commands.
- Preserve upstream license notices and record source URL, exact commit, copied files and modifications. GPUI's Apache license does not apply to Zed's GPL UI crate.
- Use semantic tokens in `design/tokens.json`. Keep desktop density and mobile touch sizing distinct. Generated images are concept references, not evidence of working UI.
- Verify Japanese IME, keyboard focus, screen-reader access, long text, empty/error states, reduced motion and realistic data volume before claiming a component is complete.
- Record target OS, device, release/debug mode, data size and measurement method with performance claims. Never report unrun builds or tests as passing.
- Keep setup and design docs in Japanese. Describe unimplemented proposals as proposals.
- Keep the GitHub repository private. Never commit credentials, real calendar data, generated build output or local environment files.
