import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Ui as Ui
import qs.Commons
import "Model.js" as Model

Ui.Panel {
  id: root
  moduleName: "wavesink.audio"
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight
  // Each monitor owns its popup; never register duplicate global IPC targets.
  readonly property string screenId: root.QsWindow.window && root.QsWindow.window.screen ? root.QsWindow.window.screen.name : ""
  ipcTarget: screenId ? "wavesink.audio." + screenId : ""
  property int restoreStage: 0
  property var meterColors: Model.palette("")
  property bool ready: false
  property string error: ""
  property var snapshot: ({mixes: [], inputs: [], profiles: []})
  property var peaks: ({inputs: {}, mixes: {}})
  property var devices: ({inputs: [], outputs: []})
  property var launchCommand: []
  property int requestId: 0
  property bool scrollPending: false
  readonly property var selectedMix: {
    var rows = snapshot.mixes || []
    for (var i = 0; i < rows.length; i++) if (rows[i].id === snapshot.bar_mix) return rows[i]
    return null
  }
  readonly property string pluginPath: decodeURIComponent(String(Qt.resolvedUrl("."))).replace(/^file:\/\//, "").replace(/\/?$/, "/")
  ListModel { id: mixes }
  ListModel { id: inputs }

  function send(method, params) {
    if (!socket.connected) return
    socket.write(JSON.stringify({id: ++requestId, method: method, params: params || {}}) + "\n")
  }
  function subscribe() { send("subscribe", {state: true, levels: opened}) }
  function showMixer() {
    if (ready) send("show_window")
    else if (launchCommand.length && !launch.running) launch.running = true
    close()
  }
  function resetScroll() {
    scroll.cancelFlick()
    scroll.contentY = 0
  }
  onOpenedChanged: {
    subscribe()
    if (opened) {
      scrollPending = !ready
      send("get_devices")
      Qt.callLater(resetScroll)
    } else {
      restoreStage = 0
      profiles.close()
      peaks = {inputs: {}, mixes: {}}
    }
  }

  FileView {
    path: root.pluginPath + "launch.json"
    watchChanges: true
    onFileChanged: reload()
    onLoaded: {
      try { root.launchCommand = JSON.parse(text()).command || [] }
      catch (e) { root.error = "Could not read WaveSink launch command" }
    }
  }
  FileView {
    path: Color.currentThemePath + "/colors.toml"
    watchChanges: true
    onFileChanged: reload()
    onLoaded: root.meterColors = Model.palette(text())
  }
  Process { id: launch; command: root.launchCommand }
  Process {
    id: restore
    command: ["python3", root.pluginPath + "integration.py", "remove"]
    stderr: StdioCollector {
      onStreamFinished: if (text.trim()) root.error = text.trim()
    }
  }
  Socket {
    id: socket
    path: Quickshell.env("XDG_RUNTIME_DIR") + "/wavesink/control.sock"
    connected: true
    onConnectionStateChanged: {
      root.ready = false
      root.peaks = {inputs: {}, mixes: {}}
      if (connected) root.subscribe()
      else {
        root.snapshot = {mixes: [], inputs: [], profiles: []}
        mixes.clear()
        inputs.clear()
        root.devices = {inputs: [], outputs: []}
      }
    }
    parser: SplitParser {
      onRead: function(line) {
        try {
          var message = JSON.parse(line)
          if (message.error) { root.error = String(message.error); return }
          if (message.event === "state") {
            var state = message.data
            if (state.protocol !== 1 || !Array.isArray(state.mixes) || !Array.isArray(state.inputs))
              throw new Error("Unsupported WaveSink control protocol")
            Model.sync(mixes, state.mixes)
            Model.sync(inputs, state.inputs)
            root.snapshot = state
            root.ready = true
            root.error = ""
            if (root.scrollPending) { root.scrollPending = false; Qt.callLater(root.resetScroll) }
          } else if (message.event === "levels") root.peaks = message.data
          else if (message.result && message.result.inputs && message.result.outputs) root.devices = message.result
        } catch (e) { root.ready = false; root.error = String(e) }
      }
    }
  }
  Timer {
    interval: 1500
    repeat: true
    running: !socket.connected
    onTriggered: socket.connected = true
  }
  Timer {
    interval: 2500
    repeat: true
    running: root.opened && root.ready
    onTriggered: root.send("get_devices")
  }

  Ui.BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: !root.ready ? "󰝟" : root.selectedMix && root.selectedMix.muted ? "󰝟" : "󰕾"
    tooltipText: !root.ready ? "WaveSink is not running" : root.selectedMix
      ? root.selectedMix.label + ": " + root.selectedMix.volume + "%" + (root.selectedMix.muted ? " (muted)" : "")
      : "WaveSink — choose a bar volume mix"
    onPressed: function(mouseButton) {
      if (mouseButton === Qt.RightButton && root.ready && root.selectedMix)
        root.send("set_mix_mute", {mix: root.selectedMix.id})
      else root.toggle()
    }
    onWheelMoved: function(delta) {
      if (root.ready && root.selectedMix)
        root.send("adjust_mix_volume", {mix: root.selectedMix.id, delta: delta > 0 ? 5 : -5})
    }
  }

  Ui.KeyboardPanel {
    id: panel
    owner: root
    anchorItem: button
    bar: root.bar
    open: root.opened
    focusTarget: mixerButton
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(content.implicitHeight + footer.implicitHeight + Style.space(8), Style.space(620))

    FocusScope {
      anchors.fill: parent
      Keys.onPressed: function(event) {
        if (confirmation.opened) event.accepted = confirmation.handleKey(event)
        else if (event.key === Qt.Key_Escape) { root.close(); event.accepted = true }
      }
      Flickable {
        id: scroll
        anchors { left: parent.left; right: parent.right; top: parent.top; bottom: footer.top; bottomMargin: Style.space(8) }
        enabled: !confirmation.opened
        clip: true
        contentWidth: width
        contentHeight: content.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        Column {
          id: content
          width: scroll.width
          spacing: Style.space(10)
          Label {
            text: "WaveSink"
            color: root.barForeground
            font.family: Style.font.family
            font.pixelSize: Style.font.title
            font.bold: true
          }
          Label {
            width: parent.width
            visible: !root.ready
            text: "WaveSink is not running."
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            color: Qt.alpha(root.barForeground, 0.7)
          }
          Label {
            visible: root.error !== ""
            width: parent.width
            text: root.error
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            color: Color.urgent
          }
          Ui.PanelSectionHeader { text: "MIXES"; foreground: root.barForeground }
          Repeater {
            model: mixes
            LevelRow {
              required property string rowId
              required property var model
              width: content.width
              enabled: root.ready
              label: model.label
              volume: model.volume
              muted: model.muted
              selectable: true
              selected: root.snapshot.bar_mix === rowId
              peak: (root.peaks.mixes || {})[rowId] || 0
              foreground: root.barForeground
              pro: !!root.snapshot.meter_pro
              meterColors: root.meterColors
              onVolumeRequested: function(value) { root.send("set_mix_volume", {mix: rowId, volume: Math.round(value)}) }
              onMuteRequested: root.send("set_mix_mute", {mix: rowId})
              onSelectRequested: root.send("set_bar_mix", {mix: selected ? null : rowId})
            }
          }
          Ui.PanelSectionHeader { visible: inputs.count > 0; text: "INPUTS"; foreground: root.barForeground }
          Repeater {
            model: inputs
            LevelRow {
              required property string rowId
              required property var model
              width: content.width
              enabled: root.ready
              label: model.label
              volume: model.volume
              muted: model.muted
              peak: (root.peaks.inputs || {})[rowId] || 0
              foreground: root.barForeground
              pro: !!root.snapshot.meter_pro
              meterColors: root.meterColors
              onVolumeRequested: function(value) { root.send("set_input_volume", {input: rowId, volume: Math.round(value)}) }
              onMuteRequested: root.send("set_input_mute", {input: rowId})
            }
          }
          Ui.PanelSectionHeader {
            visible: root.ready
            text: "AVAILABLE HARDWARE"
            foreground: root.barForeground
          }
          Repeater {
            model: (root.devices.inputs || []).concat(root.devices.outputs || [])
            Label {
              required property var modelData
              width: content.width
              text: modelData.description
              textFormat: Text.PlainText
              color: Qt.alpha(root.barForeground, 0.7)
              wrapMode: Text.WordWrap
            }
          }
        }
      }
      Column {
        id: footer
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        enabled: !confirmation.opened
        spacing: Style.space(4)
        Row {
          width: parent.width
          spacing: Style.space(8)
          Ui.Button {
            id: mixerButton
            width: profiles.visible ? (parent.width - parent.spacing) * 0.44 : parent.width
            height: profiles.implicitHeight
            text: root.ready ? "Open full mixer" : "Start WaveSink"
            foreground: root.barForeground
            focusable: true
            bordered: true
            onClicked: root.showMixer()
          }
          Ui.Dropdown {
            id: profiles
            visible: root.ready && (root.snapshot.profiles || []).length > 0
            width: (parent.width - parent.spacing) * 0.56
            label: "Profile"
            showLabel: false
            Accessible.name: "Profile"
            options: root.snapshot.profiles || []
            Binding {
              target: profiles
              property: "value"
              value: root.snapshot.active_profile || ""
              restoreMode: Binding.RestoreNone
            }
            foreground: root.barForeground
            onChanged: function(value) { root.send("load_profile", {name: value}) }
          }
        }
        Ui.Button {
          id: restoreButton
          width: parent.width
          text: "Restore OS audio panel..."
          fontSize: Style.font.caption
          verticalPadding: Style.space(3)
          foreground: root.barForeground
          focusable: true
          enabled: !restore.running
          onClicked: root.restoreStage = 1
        }
      }
      Ui.ConfirmDialog {
        id: confirmation
        anchors.fill: parent
        opened: root.restoreStage > 0
        selectedIndex: 0
        background: Color.popups.background
        foreground: root.barForeground
        message: root.restoreStage === 1
          ? "This will restore the default OS audio panel. Any previous panel customizations will be preserved."
          : "Are you sure?\n\nTo put this panel back, open WaveSink Settings > Omarchy and click Enable WaveSink panel. No reinstall is needed."
        confirmText: root.restoreStage === 1 ? "Continue" : "Restore"
        onOpenedChanged: if (opened) { selectedIndex = 0; forceActiveFocus() }
        Keys.onPressed: function(event) {
          event.accepted = event.isAutoRepeat || handleKey(event)
        }
        onCanceled: { root.restoreStage = 0; restoreButton.forceActiveFocus() }
        onConfirmed: {
          selectedIndex = 0
          if (root.restoreStage === 1) root.restoreStage = 2
          else { root.restoreStage = 0; restore.running = true }
        }
      }
    }
  }
}
