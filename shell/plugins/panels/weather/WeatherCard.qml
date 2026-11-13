import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

PanelWindow {
    id: root
    required property var host

    anchors {
        top: true
        left: true
    }

    margins.top: 32
    margins.left: 84
    implicitWidth: 200
    implicitHeight: 90
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "petra-weather-widget"
    exclusionMode: ExclusionMode.Ignore

    Row {
        anchors.left: parent.left
        anchors.top: parent.top
        spacing: 14

        Text {
            text: "☀"
            font.pixelSize: 42
            color: Color.accent
            anchors.verticalCenter: parent.verticalCenter
        }

        Column {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2

            Text {
                text: "21°C"
                font.family: Style.fontFamily
                font.pixelSize: 28
                font.bold: true
                color: Color.textPrimary
            }

            Text {
                text: "Clear • PetraOS"
                font.family: Style.fontFamily
                font.pixelSize: 13
                color: Color.textSecondary
            }
        }
    }
}
