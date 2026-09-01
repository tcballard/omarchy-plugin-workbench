import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

Panel {
  id: root
  moduleName: "io.github.tcballard.plugin-workbench"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property string helperPath: ""
  property var projects: []
  property var pluginUpdates: []
  property bool updatesChecked: false
  property bool marketplaceOpen: false
  property bool marketplaceLoaded: false
  property var marketplaceResults: []
  property int marketplaceMatched: 0
  property int marketplaceTotal: 0
  property string marketplaceGeneratedAt: ""
  property bool marketplaceBuiltInsOnly: false
  property bool marketplaceVerifiedOnly: false
  property bool marketplaceInstallableOnly: false
  property bool marketplaceInstalledOnly: false
  property string marketplaceConfirmation: ""
  property string message: ""
  property bool messageError: false
  property string refreshOutput: ""
  property string actionOutput: ""
  property string actionError: ""
  property bool showBuilderSetup: false
  property string pendingAction: ""
  readonly property int projectCount: projects.length
  readonly property int availableUpdateCount: pluginUpdates.filter(function(plugin) { return plugin.updateable }).length
  readonly property var reviewUpdates: pluginUpdates.filter(function(plugin) { return plugin.state !== "up-to-date" })
  readonly property bool busy: refreshProcess.running || actionProcess.running

  function open() {
    root.controller.show()
    refresh()
  }

  function close() {
    root.controller.hide()
  }

  function toggle() {
    if (root.opened) close()
    else open()
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.hostWidget || root, direction)
    return false
  }

  function refresh() {
    if (!root.helperPath || refreshProcess.running) return
    root.refreshOutput = ""
    refreshProcess.command = [root.helperPath, "status", "--json"]
    refreshProcess.running = true
  }

  function applyRefresh() {
    try {
      var parsed = JSON.parse(root.refreshOutput || "[]")
      root.projects = Array.isArray(parsed) ? parsed : []
      if (!Array.isArray(parsed)) {
        root.message = parsed.error || "Unexpected status response"
        root.messageError = true
      }
    } catch (error) {
      root.message = "Could not parse Workbench status: " + error
      root.messageError = true
    }
  }

  function runAction(action, projectId) {
    if (root.busy || !root.helperPath) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = action + " · " + projectId
    root.messageError = false
    root.pendingAction = "project"
    actionProcess.command = [root.helperPath, action, projectId, "--json"]
    actionProcess.running = true
  }

  function addProject() {
    var path = String(pathInput.text || "").trim()
    if (!path || root.busy) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Registering " + path
    root.messageError = false
    root.pendingAction = "project"
    actionProcess.command = [root.helperPath, "add", path, "--json"]
    actionProcess.running = true
  }

  function checkUpdates() {
    if (root.busy || !root.helperPath) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Fetching installed plugin updates…"
    root.messageError = false
    root.pendingAction = "updates"
    actionProcess.command = [root.helperPath, "updates", "--json"]
    actionProcess.running = true
  }

  function applyUpdate(pluginId, revision) {
    if (root.busy || !root.helperPath) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Updating " + pluginId + " through Omarchy…"
    root.messageError = false
    root.pendingAction = "update"
    actionProcess.command = [root.helperPath, "update", pluginId, "--revision", revision, "--yes", "--json"]
    actionProcess.running = true
  }

  function applyAllUpdates() {
    if (root.busy || !root.helperPath || root.availableUpdateCount === 0) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Applying " + root.availableUpdateCount + " reviewed update(s)…"
    root.messageError = false
    root.pendingAction = "update"
    var command = [root.helperPath, "update-all", "--yes", "--json"]
    root.pluginUpdates.filter(function(plugin) { return plugin.updateable }).forEach(function(plugin) {
      command.push("--reviewed")
      command.push(plugin.id + "=" + plugin.remoteRevision)
    })
    actionProcess.command = command
    actionProcess.running = true
  }

  function incomingSummary(commits) {
    if (!Array.isArray(commits) || commits.length === 0) return "No incoming commit summary"
    return commits.slice(0, 5).map(function(commit) {
      return commit.revision + "  " + commit.subject
    }).join("\n") + (commits.length > 5 ? "\n…" : "")
  }

  function toggleMarketplace() {
    root.marketplaceOpen = !root.marketplaceOpen
    if (root.marketplaceOpen) {
      if (root.marketplaceLoaded) searchMarketplace()
      else refreshMarketplace()
    }
  }

  function refreshMarketplace() {
    if (root.busy || !root.helperPath) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Refreshing the official marketplace catalogue…"
    root.messageError = false
    root.pendingAction = "marketplace-refresh"
    actionProcess.command = [root.helperPath, "marketplace-refresh", "--json"]
    actionProcess.running = true
  }

  function searchMarketplace() {
    if (root.busy || !root.helperPath) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Searching the cached marketplace…"
    root.messageError = false
    root.pendingAction = "marketplace-search"
    var command = [root.helperPath, "marketplace-search"]
    var query = String(marketplaceSearchInput.text || "").trim()
    if (query) command.push(query)
    if (root.marketplaceBuiltInsOnly) command.push("--built-in")
    if (root.marketplaceVerifiedOnly) command.push("--verified")
    if (root.marketplaceInstallableOnly) command.push("--installable")
    if (root.marketplaceInstalledOnly) command.push("--installed")
    command.push("--limit")
    command.push("50")
    command.push("--json")
    actionProcess.command = command
    actionProcess.running = true
  }

  function installMarketplace(plugin) {
    if (root.busy || !root.helperPath || !plugin.installable) return
    root.actionOutput = ""
    root.actionError = ""
    root.message = "Installing reviewed snapshot of " + plugin.name + "…"
    root.messageError = false
    root.pendingAction = "marketplace-install"
    actionProcess.command = [root.helperPath, "marketplace-install", plugin.id,
      "--repo", plugin.repo, "--revision", plugin.reviewedRevision,
      "--enable", "--yes", "--json"]
    actionProcess.running = true
  }

  function updateMarketplace(plugin) {
    if (root.busy || !root.helperPath || !plugin.managed || !plugin.updateAvailable) return
    root.pendingAction = "marketplace-update"
    root.message = "Applying reviewed marketplace update for " + plugin.name + "…"
    root.messageError = false
    actionProcess.command = [root.helperPath, "marketplace-update", plugin.id,
      "--revision", plugin.reviewedRevision, "--yes", "--json"]
    actionProcess.running = true
  }

  function confirmedMarketplaceAction(action, plugin) {
    var key = action + ":" + plugin.id
    if (root.marketplaceConfirmation !== key) {
      root.marketplaceConfirmation = key
      root.message = "Click “Confirm " + action + "” again for " + plugin.name
      root.messageError = false
      return
    }
    root.marketplaceConfirmation = ""
    root.pendingAction = "marketplace-" + action
    root.message = (action === "repair" ? "Repairing " : "Uninstalling ") + plugin.name + "…"
    actionProcess.command = [root.helperPath, "marketplace-" + action, plugin.id, "--yes", "--json"]
    actionProcess.running = true
  }

  function completeAction(exitCode) {
    var text = String(root.actionOutput || "").trim()
    var errorText = String(root.actionError || "").trim()
    var parsed = null
    try { parsed = JSON.parse(text || errorText || "{}") } catch (error) {}
    if (root.pendingAction === "marketplace-refresh" && exitCode === 0 && parsed && parsed.ok) {
      root.marketplaceLoaded = true
      root.marketplaceGeneratedAt = parsed.generatedAt || ""
      root.message = parsed.message || "Marketplace catalogue refreshed"
      root.messageError = false
      root.pendingAction = ""
      Qt.callLater(root.searchMarketplace)
      return
    }
    if (root.pendingAction === "marketplace-search" && exitCode === 0 && parsed && Array.isArray(parsed.plugins)) {
      root.marketplaceLoaded = true
      root.marketplaceResults = parsed.plugins
      root.marketplaceMatched = Number(parsed.matched || 0)
      root.marketplaceTotal = Number(parsed.total || 0)
      root.marketplaceGeneratedAt = parsed.generatedAt || root.marketplaceGeneratedAt
      root.message = root.marketplaceMatched + " marketplace result(s)"
      root.messageError = false
      root.pendingAction = ""
      return
    }
    if (root.pendingAction === "updates" && exitCode === 0 && parsed && Array.isArray(parsed.plugins)) {
      root.pluginUpdates = parsed.plugins
      root.updatesChecked = true
      root.message = Number(parsed.available || 0) + " update(s) available"
        + (Number(parsed.blocked || 0) > 0 ? " · " + parsed.blocked + " need attention" : "")
      root.messageError = false
      root.pendingAction = ""
      return
    }
    root.messageError = exitCode !== 0 || (parsed && parsed.ok === false)
    root.message = parsed && parsed.error ? parsed.error
      : parsed && parsed.message ? parsed.message
      : errorText || text || (exitCode === 0 ? "Action completed" : "Action failed")
    if (exitCode === 0) {
      pathInput.text = ""
      if (root.pendingAction === "update") Qt.callLater(root.checkUpdates)
      else if (root.pendingAction.indexOf("marketplace-") === 0) Qt.callLater(root.searchMarketplace)
      else Qt.callLater(root.refresh)
    }
    root.pendingAction = ""
  }

  Process {
    id: refreshProcess
    command: []
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.refreshOutput = String(text || "")
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        if (String(text || "").trim()) {
          root.message = String(text).trim()
          root.messageError = true
        }
      }
    }
    onExited: function(exitCode) {
      if (exitCode === 0) Qt.callLater(root.applyRefresh)
      else {
        root.message = root.message || "Workbench helper could not load projects"
        root.messageError = true
      }
    }
  }

  Process {
    id: actionProcess
    command: []
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.actionOutput = String(text || "")
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.actionError = String(text || "")
    }
    onExited: function(exitCode) {
      Qt.callLater(function() { root.completeAction(exitCode) })
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.hostWidget || root
    bar: root.bar
    open: root.opened
    contentWidth: panel.fittedContentWidth(Style.space(720))
    contentHeight: panel.fittedContentHeight(Style.space(540))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Rectangle {
        anchors.fill: parent
        color: root.bar ? root.bar.background : Color.background

        Column {
          anchors.fill: parent
          anchors.margins: Style.space(14)
          spacing: Style.space(10)

          Row {
            width: parent.width
            spacing: Style.space(8)

            Column {
              width: parent.width - marketplaceButton.width - refreshButton.width - updatesButton.width - Style.space(24)
              spacing: Style.space(2)

              Text {
                text: "PLUGIN WORKBENCH"
                color: root.barForeground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.title
                font.bold: true
              }
              Text {
                text: root.marketplaceOpen
                  ? root.marketplaceMatched + " of " + root.marketplaceTotal + " marketplace listings"
                  : root.projectCount + (root.projectCount === 1 ? " registered project" : " registered projects")
                color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.62)
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
              }
            }

            WorkbenchButton {
              id: marketplaceButton
              label: root.marketplaceOpen ? "Projects" : "Marketplace"
              enabled: !root.busy
              onTriggered: root.toggleMarketplace()
            }
            WorkbenchButton {
              id: updatesButton
              visible: !root.marketplaceOpen
              label: root.availableUpdateCount > 0 ? root.availableUpdateCount + " updates" : "Check updates"
              enabled: !root.busy
              onTriggered: root.checkUpdates()
            }

            WorkbenchButton {
              id: refreshButton
              label: root.busy ? "Working…" : "Refresh"
              enabled: !root.busy
              onTriggered: root.refresh()
            }
          }

          Rectangle {
            visible: !root.marketplaceOpen
            width: parent.width
            height: visible ? Style.space(38) : 0
            color: "transparent"
            border.color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.28)
            border.width: 1
            radius: Style.cornerRadius

            Row {
              anchors.fill: parent
              anchors.margins: Style.space(7)
              spacing: Style.space(8)

              TextInput {
                id: pathInput
                width: parent.width - addButton.width - Style.space(8)
                height: parent.height
                color: root.barForeground
                selectionColor: Color.accent
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.body
                verticalAlignment: TextInput.AlignVCenter
                clip: true
                selectByMouse: true
                onAccepted: root.addProject()

                Text {
                  anchors.verticalCenter: parent.verticalCenter
                  visible: !pathInput.text
                  text: "Absolute path to a local plugin project"
                  color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.42)
                  font: pathInput.font
                }
              }

              WorkbenchButton {
                id: addButton
                label: "Register"
                enabled: !root.busy && String(pathInput.text || "").trim() !== ""
                onTriggered: root.addProject()
              }
            }
          }

          Rectangle {
            visible: root.marketplaceOpen
            width: parent.width
            height: visible ? Style.space(38) : 0
            color: "transparent"
            border.color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.28)
            border.width: 1
            radius: Style.cornerRadius

            Row {
              anchors.fill: parent
              anchors.margins: Style.space(7)
              spacing: Style.space(8)

              TextInput {
                id: marketplaceSearchInput
                width: parent.width - marketplaceSearchButton.width - marketplaceRefreshButton.width - Style.space(16)
                height: parent.height
                color: root.barForeground
                selectionColor: Color.accent
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.body
                verticalAlignment: TextInput.AlignVCenter
                clip: true
                selectByMouse: true
                onAccepted: root.searchMarketplace()

                Text {
                  anchors.verticalCenter: parent.verticalCenter
                  visible: !marketplaceSearchInput.text
                  text: "Search name, description, author, category or tag"
                  color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.42)
                  font: marketplaceSearchInput.font
                }
              }

              WorkbenchButton {
                id: marketplaceSearchButton
                label: "Search"
                enabled: !root.busy && root.marketplaceLoaded
                onTriggered: root.searchMarketplace()
              }

              WorkbenchButton {
                id: marketplaceRefreshButton
                label: "Refresh"
                enabled: !root.busy
                onTriggered: root.refreshMarketplace()
              }
            }
          }

          Row {
            visible: root.marketplaceOpen
            height: visible ? implicitHeight : 0
            spacing: Style.space(6)
            WorkbenchButton {
              label: root.marketplaceBuiltInsOnly ? "Built-ins ✓" : "Built-ins"
              onTriggered: {
                root.marketplaceBuiltInsOnly = !root.marketplaceBuiltInsOnly
                root.searchMarketplace()
              }
            }
            WorkbenchButton {
              label: root.marketplaceVerifiedOnly ? "Verified ✓" : "Verified"
              onTriggered: {
                root.marketplaceVerifiedOnly = !root.marketplaceVerifiedOnly
                root.searchMarketplace()
              }
            }
            WorkbenchButton {
              label: root.marketplaceInstallableOnly ? "Installable ✓" : "Installable"
              onTriggered: {
                root.marketplaceInstallableOnly = !root.marketplaceInstallableOnly
                root.searchMarketplace()
              }
            }
            WorkbenchButton {
              label: root.marketplaceInstalledOnly ? "Installed ✓" : "Installed"
              onTriggered: {
                root.marketplaceInstalledOnly = !root.marketplaceInstalledOnly
                root.searchMarketplace()
              }
            }
            Text {
              anchors.verticalCenter: parent.verticalCenter
              text: root.marketplaceGeneratedAt ? "Catalogue " + root.marketplaceGeneratedAt.slice(0, 10) : ""
              color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.48)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
            }
          }

          Rectangle {
            width: parent.width
            height: root.message ? Style.space(34) : 0
            visible: height > 0
            radius: Style.cornerRadius
            color: root.messageError
              ? Qt.rgba(0.72, 0.18, 0.18, 0.20)
              : Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.14)

            Text {
              anchors.fill: parent
              anchors.margins: Style.space(8)
              text: root.message
              color: root.messageError ? Color.urgent : root.barForeground
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
              elide: Text.ElideRight
              verticalAlignment: Text.AlignVCenter
            }
          }

          Flickable {
            width: parent.width
            height: parent.height - y
            contentWidth: width
            contentHeight: projectList.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
              id: projectList
              width: parent.width
              spacing: Style.space(8)

              Column {
                visible: root.marketplaceOpen
                width: parent.width
                spacing: Style.space(8)

                Repeater {
                  model: root.marketplaceResults
                  delegate: MarketplaceCard {
                    required property var modelData
                    width: projectList.width
                    plugin: modelData
                  }
                }

                Text {
                  visible: root.marketplaceLoaded && root.marketplaceResults.length === 0
                  width: parent.width
                  topPadding: Style.space(40)
                  text: "No marketplace plugins match this search."
                  color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.body
                  horizontalAlignment: Text.AlignHCenter
                }
              }

              Rectangle {
                visible: !root.marketplaceOpen && root.updatesChecked
                width: parent.width
                height: visible ? updateContent.implicitHeight + Style.space(20) : 0
                color: Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.08)
                radius: Style.cornerRadius
                border.color: Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.28)
                border.width: 1

                Column {
                  id: updateContent
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.top: parent.top
                  anchors.margins: Style.space(10)
                  spacing: Style.space(8)

                  Row {
                    width: parent.width
                    spacing: Style.space(8)

                    Column {
                      width: parent.width - updateAllButton.width - Style.space(8)
                      spacing: Style.space(2)
                      Text {
                        text: "INSTALLED PLUGIN UPDATES"
                        color: root.barForeground
                        font.family: root.bar ? root.bar.fontFamily : Style.font.family
                        font.pixelSize: Style.font.body
                        font.bold: true
                      }
                      Text {
                        text: root.availableUpdateCount > 0
                          ? root.availableUpdateCount + " fast-forward update(s) ready after review"
                          : root.reviewUpdates.length > 0
                            ? root.reviewUpdates.length + " plugin(s) need attention"
                            : "All Git-managed plugins are up to date"
                        color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
                        font.family: root.bar ? root.bar.fontFamily : Style.font.family
                        font.pixelSize: Style.font.caption
                      }
                    }

                    WorkbenchButton {
                      id: updateAllButton
                      visible: root.availableUpdateCount > 0
                      label: "Update all"
                      enabled: !root.busy
                      onTriggered: root.applyAllUpdates()
                    }
                  }

                  Repeater {
                    model: root.reviewUpdates
                    delegate: UpdateCard {
                      required property var modelData
                      width: updateContent.width
                      update: modelData
                    }
                  }
                }
              }

              Repeater {
                model: root.marketplaceOpen ? [] : root.projects

                delegate: ProjectCard {
                  required property var modelData
                  width: projectList.width
                  project: modelData
                }
              }

              Column {
                visible: !root.marketplaceOpen && root.projects.length === 0
                width: parent.width
                topPadding: Style.space(48)
                spacing: Style.space(9)

                Text {
                  width: parent.width
                  text: "Create with Build Omarchy Plugins"
                  color: root.barForeground
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.body
                  font.bold: true
                  horizontalAlignment: Text.AlignHCenter
                }

                Text {
                  width: parent.width
                  text: "Use the agent companion to scaffold a project, then register its checkout here. Workbench never scans your home directory automatically."
                  color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.body
                  horizontalAlignment: Text.AlignHCenter
                  wrapMode: Text.WordWrap
                }

                WorkbenchButton {
                  anchors.horizontalCenter: parent.horizontalCenter
                  label: root.showBuilderSetup ? "Hide setup" : "Builder setup"
                  onTriggered: root.showBuilderSetup = !root.showBuilderSetup
                }

                Rectangle {
                  visible: root.showBuilderSetup
                  width: parent.width
                  height: visible ? setupText.implicitHeight + Style.space(20) : 0
                  radius: Style.cornerRadius
                  color: Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.10)

                  Text {
                    id: setupText
                    anchors.fill: parent
                    anchors.margins: Style.space(10)
                    text: "Install Build Omarchy Plugins in your preferred agent host, ask it to scaffold an Omarchy plugin, then paste the generated checkout path above.\n\ngithub.com/tcballard/build-omarchy-plugins"
                    color: root.barForeground
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.caption
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                  }
                }
              }
            }
          }
        }
      }
    }
  }

  component WorkbenchButton: Rectangle {
    id: actionButton
    property string label: ""
    signal triggered()
    implicitWidth: buttonLabel.implicitWidth + Style.space(18)
    implicitHeight: Style.space(28)
    radius: Style.cornerRadius
    color: buttonHover.hovered
      ? Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.30)
      : Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.09)
    opacity: enabled ? 1 : 0.45

    Text {
      id: buttonLabel
      anchors.centerIn: parent
      text: actionButton.label
      color: root.barForeground
      font.family: root.bar ? root.bar.fontFamily : Style.font.family
      font.pixelSize: Style.font.caption
      font.bold: true
    }
    HoverHandler { id: buttonHover }
    TapHandler { enabled: actionButton.enabled; onTapped: actionButton.triggered() }
  }

  component ProjectCard: Rectangle {
    id: card
    required property var project
    implicitHeight: cardContent.implicitHeight + Style.space(20)
    color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.055)
    radius: Style.cornerRadius
    border.color: project.deployment === "drifted"
      ? Color.urgent : Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.12)
    border.width: 1

    Column {
      id: cardContent
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      anchors.margins: Style.space(10)
      spacing: Style.space(7)

      Row {
        width: parent.width
        spacing: Style.space(8)

        Column {
          width: parent.width - stateText.width - Style.space(8)
          spacing: Style.space(2)

          Text {
            width: parent.width
            text: card.project.name || card.project.id
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            font.bold: true
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            text: Number(card.project.workflows || 0) + " workflows  ·  "
              + Number(card.project.activeSessions || 0) + " active sessions"
              + (Number(card.project.activeTestSessions || 0) > 0
                ? "  ·  NESTED TEST RUNNING" : "")
              + (card.project.definitionChangedSinceTrust ? "  ·  TRUST STALE" : "")
            color: card.project.definitionChangedSinceTrust ? Color.urgent
              : Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.48)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            text: card.project.id + "  ·  "
              + (card.project.revision ? String(card.project.revision).slice(0, 10) : "no git revision")
              + (card.project.dirty ? "  ·  DIRTY" : "")
            color: card.project.dirty ? Color.urgent
              : Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            elide: Text.ElideRight
          }
        }

        Text {
          id: stateText
          text: String(card.project.deployment || "unknown").toUpperCase()
          color: card.project.deployment === "snapshot" ? Color.accent : root.barForeground
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.caption
          font.bold: true
        }
      }

      Row {
        spacing: Style.space(5)
        WorkbenchButton { label: "Validate"; onTriggered: root.runAction("validate", card.project.id) }
        WorkbenchButton {
          visible: Number(card.project.checks || 0) + Number(card.project.workflows || 0)
            + Number(card.project.environmentRequirements || 0) > 0
          label: card.project.projectChecksTrusted ? "Untrust" : "Trust commands"
          onTriggered: root.runAction(card.project.projectChecksTrusted ? "untrust" : "trust", card.project.id)
        }
        WorkbenchButton { label: "Test"; onTriggered: root.runAction("check", card.project.id) }
        WorkbenchButton {
          label: Number(card.project.activeTestSessions || 0) > 0 ? "Stop test window" : "Test window"
          onTriggered: root.runAction(
            Number(card.project.activeTestSessions || 0) > 0
              ? "test-session-stop" : "test-session-start",
            card.project.id)
        }
        WorkbenchButton { label: "Diagnose"; onTriggered: root.runAction("diagnose", card.project.id) }
        WorkbenchButton { label: "Release check"; onTriggered: root.runAction("release-check", card.project.id) }
        WorkbenchButton { label: "Live link"; onTriggered: root.runAction("link", card.project.id) }
        WorkbenchButton { label: "Snapshot"; onTriggered: root.runAction("snapshot", card.project.id) }
        WorkbenchButton { label: "Rollback"; onTriggered: root.runAction("rollback", card.project.id) }
        WorkbenchButton {
          label: card.project.enabled === true ? "Disable" : "Enable"
          onTriggered: root.runAction(card.project.enabled === true ? "disable" : "enable", card.project.id)
        }
      }
    }
  }

  component UpdateCard: Rectangle {
    id: updateCard
    required property var update
    implicitHeight: updateCardContent.implicitHeight + Style.space(16)
    color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.055)
    radius: Style.cornerRadius
    border.color: update.updateable
      ? Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.45)
      : Qt.rgba(Color.urgent.r, Color.urgent.g, Color.urgent.b, 0.42)
    border.width: 1

    Column {
      id: updateCardContent
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      anchors.margins: Style.space(8)
      spacing: Style.space(5)

      Row {
        width: parent.width
        spacing: Style.space(8)
        Column {
          width: parent.width - updateButton.width - Style.space(8)
          spacing: Style.space(2)
          Text {
            width: parent.width
            text: updateCard.update.id
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            font.bold: true
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            text: String(updateCard.update.state || "unknown").replace(/-/g, " ").toUpperCase()
              + (Number(updateCard.update.behind || 0) > 0 ? " · " + updateCard.update.behind + " incoming" : "")
              + (Number(updateCard.update.ahead || 0) > 0 ? " · " + updateCard.update.ahead + " local" : "")
            color: updateCard.update.updateable ? Color.accent : Color.urgent
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            font.bold: true
          }
        }
        WorkbenchButton {
          id: updateButton
          visible: updateCard.update.updateable
          label: "Update"
          enabled: !root.busy
          onTriggered: root.applyUpdate(updateCard.update.id, updateCard.update.remoteRevision)
        }
      }

      Text {
        visible: updateCard.update.currentRevision && updateCard.update.remoteRevision
        width: parent.width
        text: String(updateCard.update.currentRevision || "").slice(0, 10)
          + " → " + String(updateCard.update.remoteRevision || "").slice(0, 10)
        color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
      }

      Text {
        visible: updateCard.update.commits && updateCard.update.commits.length > 0
        width: parent.width
        text: root.incomingSummary(updateCard.update.commits)
        color: root.barForeground
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        wrapMode: Text.Wrap
      }

      Text {
        visible: Boolean(updateCard.update.diffStat)
        width: parent.width
        text: updateCard.update.diffStat || ""
        color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.62)
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        wrapMode: Text.Wrap
        maximumLineCount: 6
        elide: Text.ElideRight
      }

      Text {
        visible: Boolean(updateCard.update.error)
        width: parent.width
        text: updateCard.update.error || ""
        color: Color.urgent
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        wrapMode: Text.Wrap
        maximumLineCount: 4
        elide: Text.ElideRight
      }
    }
  }

  component MarketplaceCard: Rectangle {
    id: marketplaceCard
    required property var plugin
    implicitHeight: marketplaceCardContent.implicitHeight + Style.space(18)
    color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.055)
    radius: Style.cornerRadius
    border.color: plugin.installed
      ? Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.42)
      : Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.12)
    border.width: 1

    Column {
      id: marketplaceCardContent
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      anchors.margins: Style.space(9)
      spacing: Style.space(5)

      Row {
        width: parent.width
        spacing: Style.space(8)
        Column {
          width: parent.width - marketplaceActions.width - Style.space(8)
          spacing: Style.space(2)
          Text {
            width: parent.width
            text: marketplaceCard.plugin.name + "  ·  " + marketplaceCard.plugin.version
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            font.bold: true
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            text: marketplaceCard.plugin.kind + "  ·  " + marketplaceCard.plugin.category
              + "  ·  " + marketplaceCard.plugin.author
            color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.58)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            elide: Text.ElideRight
          }
        }
        Row {
          id: marketplaceActions
          spacing: Style.space(5)
          WorkbenchButton {
            visible: marketplaceCard.plugin.installable
            label: "Install & enable"
            enabled: !root.busy
            onTriggered: root.installMarketplace(marketplaceCard.plugin)
          }
          WorkbenchButton {
            visible: marketplaceCard.plugin.managed && marketplaceCard.plugin.updateAvailable
            label: "Update"
            enabled: !root.busy
            onTriggered: root.updateMarketplace(marketplaceCard.plugin)
          }
          WorkbenchButton {
            visible: marketplaceCard.plugin.managed
            label: root.marketplaceConfirmation === "repair:" + marketplaceCard.plugin.id
              ? "Confirm repair" : "Repair"
            enabled: !root.busy
            onTriggered: root.confirmedMarketplaceAction("repair", marketplaceCard.plugin)
          }
          WorkbenchButton {
            visible: marketplaceCard.plugin.managed
            label: root.marketplaceConfirmation === "uninstall:" + marketplaceCard.plugin.id
              ? "Confirm uninstall" : "Uninstall"
            enabled: !root.busy
            onTriggered: root.confirmedMarketplaceAction("uninstall", marketplaceCard.plugin)
          }
        }
      }

      Text {
        width: parent.width
        text: marketplaceCard.plugin.description
        color: root.barForeground
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        wrapMode: Text.WordWrap
        maximumLineCount: 3
        elide: Text.ElideRight
      }

      Text {
        width: parent.width
        text: marketplaceCard.plugin.id
          + (marketplaceCard.plugin.tags.length ? "  ·  " + marketplaceCard.plugin.tags.join(", ") : "")
        color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.52)
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
      }

      Text {
        width: parent.width
        text: marketplaceCard.plugin.builtIn ? "BUILT IN"
          : marketplaceCard.plugin.managed
            ? "WORKBENCH MANAGED"
              + (marketplaceCard.plugin.updateAvailable ? "  ·  UPDATE AVAILABLE" : "")
          : marketplaceCard.plugin.installed ? "INSTALLED"
          : String(marketplaceCard.plugin.verificationStatus || "unverified").toUpperCase()
            + (marketplaceCard.plugin.reviewedRevision
              ? "  ·  REVIEWED " + String(marketplaceCard.plugin.reviewedRevision).slice(0, 10)
              : "")
        color: marketplaceCard.plugin.installed || marketplaceCard.plugin.verificationStatus === "verified"
          ? Color.accent : Color.urgent
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        font.bold: true
      }

      Text {
        width: parent.width
        text: marketplaceCard.plugin.repo
        color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.46)
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.caption
        elide: Text.ElideMiddle
      }
    }
  }
}
