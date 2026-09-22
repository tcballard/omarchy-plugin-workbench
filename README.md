<h1 align="center">Plugin Workbench for Omarchy</h1>

<p align="center">
  <a href="https://github.com/tcballard/omarchy-badges"><img src="https://raw.githubusercontent.com/tcballard/omarchy-badges/75975e5b5bf75e7ede3764bcd2950046f7abfe2c/badges/v1/omarchy-plugin.svg" alt="Built for Omarchy: Plugin" height="24"></a>
</p>

**Find, manage and build your Omarchy plugins.**

Plugin Workbench brings discovery, installed plugins, updates and development into one native Omarchy panel. Browse the marketplace, review incoming changes and keep your own plugin projects close at hand.

## Everyday use

**Discover** searches the cached marketplace. **Installed** shows what Omarchy has loaded and how it is managed. **Updates** lets you review exact revisions before applying them. **Build** gives your own projects a place to validate, test and prepare for release.

## Install

Compatible Omarchy Quattro on x86_64 and Rust 1.98.0 for the local helper build. [Compatibility and acceptance →](GUIDE.md#status)

For development, clone and build the helper:

```bash
git clone https://github.com/tcballard/omarchy-plugin-workbench.git
cd omarchy-plugin-workbench
cargo build --workspace --locked --release
```

Then [link the built checkout into Omarchy](GUIDE.md#load-it-while-developing) and open its bar item. A live link keeps the local helper build available. The [launch guide](GUIDE.md#launch-the-native-plugin-panel) includes an optional keyboard shortcut.

## Update and remove

This plugin also has a native runtime. Follow the [runtime and plugin instructions](GUIDE.md#load-it-while-developing) when updating or removing it. The shell plugin can be removed with `omarchy plugin remove io.github.tcballard.plugin-workbench`.

## A few useful details

**0.3.0 development line.** Native visual and interaction acceptance remains a release gate. First-party packaging is proposed, not assumed available. [Live acceptance](docs/live-acceptance.md)

Workbench retains project registration and deployment history. Package-owned plugins are updated through package management. [State and recovery](GUIDE.md#state-and-recovery) · [Security](SECURITY.md)

[Usage and development guide](GUIDE.md) · [Report a bug](https://github.com/tcballard/omarchy-plugin-workbench/issues)

[MIT licensed](LICENSE).

<!-- Preserve links to sections now in the guide. -->
<a id="build-omarchy-plugins-companion"></a>
<a id="build-the-local-package"></a>
<a id="cli-json-contract"></a>
<a id="create-a-personal-plugin"></a>
<a id="disposable-nested-test-window"></a>
<a id="evidence-and-release-readiness"></a>
<a id="exact-commit-security-review"></a>
<a id="launch-the-native-plugin-panel"></a>
<a id="load-it-while-developing"></a>
<a id="parallel-agent-sessions-and-handoffs"></a>
<a id="portable-project-contract"></a>
<a id="release-and-marketplace-submission-preparation"></a>
<a id="review-and-apply-installed-plugin-updates"></a>
<a id="search-and-install-official-marketplace-listings"></a>
<a id="security"></a>
<a id="state-and-recovery"></a>
<a id="status"></a>
<a id="verification"></a>
<a id="what-it-manages"></a>

[Looking for the previous detailed sections? Open the full guide →](GUIDE.md)
