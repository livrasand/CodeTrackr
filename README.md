# CodeTrackr

[![Get it on VSCode](static/images/get-it-on-vscode.svg)](https://marketplace.visualstudio.com/items?itemName=livrasand.codetrackr-official) [![Get it on Open VSX](static/images/get-it-on-open-vsx.svg)](https://open-vsx.org/extension/livrasand/codetrackr-official)

A free, open source, privacy-focused, self-hosted programming time tracker. Record how much time you spend programming, on which projects, with which languages and editors.

The backend is written in Rust using the Axum framework, with PostgreSQL as the database, Redis for real-time data and cache, and a vanilla JavaScript frontend with native ES modules.

## How does it work?

IDE extensions (VS Code, Neovim, etc.) send heartbeats via POST to /api/v1/heartbeat with data such as project, file, language, and editor. The backend stores them and adds statistics in real time.

Users see their activity on a live dashboard connected via WebSocket. There are weekly global leaderboards stored in Redis, and a JavaScript Plugin system that allows the community to add panels to the dashboard without recompiling anything, or share their plugins for free in the Plugin Store, running in a QuickJS sandbox on the server or directly in the browser.

It also has customizable CSS themes, authentication via GitHub/GitLab/Anonymous accounts, data export, and support for Stripe, for the Pro Cloud plan.

### Works on:

| <img src="static/images/ides/vs-code-128.png" width="64" alt="VS Code"><br><strong>VS Code</strong> | <img src="static/images/ides/cursor-128.png" width="64" alt="Cursor"><br><strong>Cursor</strong> | <img src="static/images/ides/windsurf-128.png" width="64" alt="Windsurf"><br><strong>Windsurf</strong> | <img src="static/images/ides/codium_cnl.svg" width="64" alt="VSCodium"><br><strong>VSCodium</strong> |
| --- | --- | --- | --- |
| <img src="static/images/ides/codesandbox_12998_logo_1631778366_kenkz.png.avif" width="64" alt="CodeSandbox"><br><strong>CodeSandbox</strong> | <img src="static/images/ides/eclipse-128.png" width="64" alt="Eclipse"><br><strong>Eclipse</strong> | <img src="static/images/ides/117817022.png" width="64" alt="Gitpod"><br><strong>Gitpod</strong> | <img src="static/images/ides/28635252.jpeg" width="64" alt="StackBlitz"><br><strong>StackBlitz</strong> |
| <img src="static/images/ides/antigravity-128.png" width="64" alt="Antigravity"><br><strong>Antigravity</strong> | <img src="static/images/ides/azure-data-studio-128.png" width="64" alt="Azure Data Studio"><br><strong>Azure Data Studio</strong> | <img src="static/images/ides/opencode-128.png" width="64" alt="OpenCode"><br><strong>OpenCode</strong> | <img src="static/images/ides/trae-128.png" width="64" alt="Trae"><br><strong>Trae</strong> |

All product names, logos, and brands are property of their respective owners.

## Official Extensions

These are the official extensions recognized and maintained by the project:

- **VS Code:** [livrasand/codetrackr-vscode](https://github.com/livrasand/codetrackr-vscode) — Track your coding activity in Visual Studio Code.
- **Unity:** [livrasand/codetrackr-unity](https://github.com/livrasand/codetrackr-unity) - Track your development time in Unity.

---

## Built by the community, for the community

CodeTrackr is designed from the ground up to grow with the people who use it. We don't want CodeTrackr — or its code — to be limited by us. The community shapes what CodeTrackr becomes.

You don't need to fork this repo or contribute code directly to make CodeTrackr yours. You can:

- **Build an IDE extension** for any editor you use — if it can make an HTTP request, it can send heartbeats. [See the IDE Integration docs →](https://codetrackr.fly.dev/docs#creating-extensions)
- **Create a dashboard plugin** — add any panel, chart, or widget to the CodeTrackr UI using plain JavaScript, no build step required. Share it in the Plugin Store for free. [See the Plugin docs →](https://codetrackr.fly.dev/docs#widget-plugins)
- **Publish a theme** — customize every color and share your look with the community. [See the Themes docs →](https://codetrackr.fly.dev/docs#themes-overview)

> _CodeTrackr is not a product you use. It's infrastructure you own and extend._

The full [official documentation](https://codetrackr.fly.dev/docs) covers everything: IDE extension APIs, plugin development, lifecycle hooks, the theme system, and the full REST API reference.

---

## Security

CodeTrackr is an open platform — and that comes with responsibility. This section documents known security considerations and the current state of mitigations.

### Plugin system

Dashboard plugins run as plain JavaScript inside a `new Function()` call in the user's browser. The script receives a `container` DOM element and a `token` scoped to the authenticated user. While plugins cannot access application-level variables, they do run in the same browser context with full access to standard Web APIs.

**Current priority: `eval` and dynamic code execution in IDE extensions**

The risk we are most focused on right now is the possibility of someone publishing an IDE extension — or a dashboard plugin — that uses `eval()`, `Function()`, or similar dynamic execution patterns to run arbitrary or obfuscated code. This is a known attack vector that could be used to exfiltrate API keys or tokens from users who install a malicious extension.

We are actively working to address this. Possible mitigations under consideration include:

- Statically analyzing plugin scripts server-side before they are accepted into the Plugin Store
- Blocking or flagging submissions that contain `eval`, `Function(`, `setTimeout(string`, `setInterval(string`, or dynamic `import()`
- Restricting certain JavaScript syntax patterns at publish time to reduce the attack surface for core security

> This is an evolving area. If you discover a security vulnerability, please report it privately before disclosing it publicly.

### Lifecycle plugins (QuickJS sandbox)

Server-side lifecycle plugins run inside an isolated QuickJS sandbox with no network, filesystem, or OS access. Memory is capped at 16 MB and execution times out at 15 seconds. SQL access is restricted to a whitelist of tables and allowed commands only. These constraints significantly limit the blast radius of a malicious lifecycle plugin.

### Responsible disclosure

If you find a security issue in CodeTrackr, please open a private issue or contact the maintainer directly before making it public. We take security reports seriously and will respond promptly.

---

## A note on AI-assisted development

CodeTrackr began as an experiment to evaluate the current limits of artificial intelligence as a software development assistant, using Claude as the primary code generation tool. This README documents the observations from that process.

### Methodology

The project was developed under a human-AI collaboration approach where:

- **~77% of the code** was generated by Claude from detailed, specific prompts.
- **~23% of the effort** consisted of human review: security auditing, vulnerability fixes, architectural adjustments, and business logic validation.
- Every generated fragment was reviewed, understood, and adapted before being integrated into the codebase.

### Observations

1. **AI does not replace technical judgment.** An effective prompt requires the human developer to possess deep domain knowledge: architecture, security, state management, network protocols, and systems design. AI executes; the human directs and validates.

2. **Time is the primary benefit.** AI drastically reduces the time spent writing boilerplate code, routine implementations, and exploring alternatives. What would traditionally have taken weeks was completed in days.

3. **Human supervision is irreplaceable.** AI does not perform security audits, does not understand the full business context, and cannot anticipate vulnerabilities in component interactions. Every generated line was inspected for security flaws, logic errors, and performance issues.

### Conclusion

Artificial intelligence is a productivity multiplier, not a substitute for the developer. Projects like CodeTrackr demonstrate that well-structured human-AI collaboration can achieve high-quality technical results in a fraction of the time, as long as the developer maintains architectural control and responsibility over the produced code.


