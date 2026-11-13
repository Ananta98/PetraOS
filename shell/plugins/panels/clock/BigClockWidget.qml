import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

PanelWindow {
    id: root
    required property var host

    anchors {
        bottom: true
        right: true
    }

    margins.bottom: 32
    margins.right: 84
    implicitWidth: 320
    implicitHeight: 130
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "petra-clock-widget"
    exclusionMode: ExclusionMode.Ignore

    property date now: new Date()

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: root.now = new Date()
    }

    Column {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        spacing: 2

        Text {
            anchors.right: parent.right
            text: Util.formatTime(root.now)
            font.family: Style.fontFamily
            font.pixelSize: Style.fontSizeClockHuge
            font.bold: true
            color: Color.textPrimary
        }

        Text {
            anchors.right: parent.right
            text: Util.formatDateLong(root.now)
            font.family: Style.fontFamily
            font.pixelSize: 18
            font.weight: Font.Medium
            color: Color.textSecondary
        }
    }
}
