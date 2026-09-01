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
  property string message: ""
  property bool messageError: false
  property string refreshOutput: ""
  property string actionOutput: ""
  property string actionError: ""
  property bool showBuilderSetup: false
  readonly property int projectCount: projects.length
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
    actionProcess.command = [root.helperPath, "add", path, "--json"]
    actionProcess.running = true
  }

  function completeAction(exitCode) {
    var text = String(root.actionOutput || "").trim()
    var errorText = String(root.actionError || "").trim()
    var parsed = null
    try { parsed = JSON.parse(text || errorText || "{}") } catch (error) {}
    root.messageError = exitCode !== 0 || (parsed && parsed.ok === false)
    root.message = parsed && parsed.error ? parsed.error
      : parsed && parsed.message ? parsed.message
      : errorText || text || (exitCode === 0 ? "Action completed" : "Action failed")
    if (exitCode === 0) {
      pathInput.text = ""
      Qt.callLater(root.refresh)
    }
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
              width: parent.width - refreshButton.width - Style.space(8)
              spacing: Style.space(2)

              Text {
                text: "PLUGIN WORKBENCH"
                color: root.barForeground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.title
                font.bold: true
              }
              Text {
                text: root.projectCount + (root.projectCount === 1 ? " registered project" : " registered projects")
                color: Qt.rgba(root.barForeground.r, root.barForeground.g, root.barForeground.b, 0.62)
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
              }
            }

            WorkbenchButton {
              id: refreshButton
              label: root.busy ? "Working…" : "Refresh"
              enabled: !root.busy
              onTriggered: root.refresh()
            }
          }

          Rectangle {
            width: parent.width
            height: Style.space(38)
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

              Repeater {
                model: root.projects

                delegate: ProjectCard {
                  required property var modelData
                  width: projectList.width
                  project: modelData
                }
              }

              Column {
                visible: root.projects.length === 0
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
}
