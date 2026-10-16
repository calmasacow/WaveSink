import QtQuick
import QtQuick.Controls
import qs.Ui as Ui
import qs.Commons
import "Model.js" as Model

Column {
  id: root
  required property string label
  required property real volume
  required property bool muted
  property bool pro: false
  property var meterColors: Model.palette("")
  property real peak: 0
  property bool selected: false
  property bool selectable: false
  property color foreground: Color.foreground
  signal volumeRequested(real value)
  signal muteRequested()
  signal selectRequested()
  spacing: Style.space(3)

  Row {
    id: heading
    width: parent.width
    spacing: Style.space(6)
    Label {
      text: root.label
      textFormat: Text.PlainText
      color: root.foreground
      font.family: Style.font.family
      font.pixelSize: Style.font.body
      width: parent.width - muteButton.width - selectButton.width - parent.spacing * 2
      elide: Text.ElideRight
      anchors.verticalCenter: parent.verticalCenter
    }
    Ui.Button {
      id: selectButton
      foreground: root.foreground
      focusable: true
      selected: root.selected
      visible: root.selectable
      width: visible ? implicitWidth : 0
      text: root.selected ? "Bar ✓" : "Bar"
      Accessible.name: "Use " + root.label + " for bar volume"
      onClicked: root.selectRequested()
    }
    Ui.Button {
      id: muteButton
      foreground: root.foreground
      focusable: true
      selected: root.muted
      text: root.muted ? "Unmute" : "Mute"
      Accessible.name: text + " " + root.label
      onClicked: root.muteRequested()
    }
  }
  Slider {
    id: control
    width: parent.width
    from: 0
    to: 100
    stepSize: 1
    wheelEnabled: false
    Accessible.name: root.label + " volume"
    onMoved: root.volumeRequested(value)
    Binding {
      target: control
      property: "value"
      value: root.volume
      when: !control.pressed
      restoreMode: Binding.RestoreNone
    }
    background: Rectangle {
      x: control.leftPadding
      y: control.topPadding + control.availableHeight / 2 - height / 2
      width: control.availableWidth
      height: Style.space(6)
      radius: height / 2
      color: Qt.alpha(root.foreground, 0.15)
      Item {
        width: parent.width * (root.muted ? 0 : Model.meter(root.peak, root.pro))
        height: parent.height
        clip: true
        Rectangle {
          width: control.availableWidth
          height: parent.height
          radius: height / 2
          gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: root.meterColors.green }
            GradientStop { position: root.pro ? 0.7 : 0.4; color: root.meterColors.green }
            GradientStop { position: root.pro ? 0.8 : 0.75; color: root.meterColors.yellow }
            GradientStop { position: root.pro ? 0.949999 : 0.95; color: root.pro ? root.meterColors.yellow : root.meterColors.red }
            GradientStop { position: 0.95; color: root.meterColors.red }
            GradientStop { position: 1; color: root.meterColors.red }
          }
        }
        Behavior on width { NumberAnimation { duration: 70 } }
      }
    }
    ToolTip.visible: hovered || pressed || activeFocus
    ToolTip.text: Math.round(value) + "%"
  }
}
