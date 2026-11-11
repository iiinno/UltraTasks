# UltraTasks development

- Do not run Rust builds, cargo check/test/clippy/bench, fixture generation, or heavy performance measurements on the user's development PC. These workloads made the PC unusable. Run them on a GitHub-hosted Windows runner or an explicitly selected remote build host. Do not work around this rule with fewer local jobs or background execution. Local work is limited to editing, lightweight inspection, and user-authorized execution of already-built artifacts.
- Implement the system first according to `docs/implementation-plan.md`. The user will supervise UI polish later. Keep GPUI views simple and replaceable; visual design approval is not a prerequisite for core implementation. Reuse suitable OSS behavior before writing new primitives.
- Initial scope is personal use across the user's own devices. External calendars come after standalone import/export. The self-hosted server is trusted to read data; authentication and transport protection are required, E2EE is not an initial requirement.
- Start with a headless Rust core and a thin desktop app. Keep SQLite in its own core module and model/calendar calculations independent of persistence. Add server, Android and MCP hosts when their implementation stage needs them; avoid unused abstraction layers.
- The initial product is calendar + ToDo. Keep documents, kanban, AI and MCP outside the first implementation slice unless explicitly requested.
- Product clients must be native Rust/GPUI with no WebView. Do not silently substitute Electron, Tauri, React or a website. Android feasibility is an open spike; report evidence and blockers.
- Keep local data operations independent of network availability. Task, Event and TimeBlock have distinct semantics.
- Route user edits through shared application commands with validation and undo. Future AI/MCP uses the same commands.
- Preserve upstream license notices and record source URL, exact commit, copied files and modifications. GPUI's Apache license does not apply to Zed's GPL UI crate.
- Use semantic tokens in `design/tokens.json`. Keep desktop density and mobile touch sizing distinct. Generated images are concept references, not evidence of working UI.
- Add only minimal configurable GPUI animations for performance testing: a short fade and position interpolation. Provide an off/reduced-motion path, and stop redraw requests when animations finish. Keep layout, presentation state, theme and motion separate from core data.
- Verify Japanese IME, keyboard focus, screen-reader access, long text, empty/error states, reduced motion and realistic data volume before claiming a component is complete.
- Record target OS, device, release/debug mode, data size and measurement method with performance claims. Never report unrun builds or tests as passing.
- Keep setup and design docs in Japanese. Describe unimplemented proposals as proposals.
- The user authorized making this repository public after a security and secret review of the working tree, history, and GitHub-visible material. Keep it private until that review has no unresolved publication blocker. Never commit credentials, real calendar data, generated build output or local environment files.
