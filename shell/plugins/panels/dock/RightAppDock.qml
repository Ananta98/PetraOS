import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui

PanelWindow {
    id: root
    required property var host

    anchors {
        right: true
    }

    margins.right: 12
    implicitWidth: Style.dockWidth
    implicitHeight: dockColumn.implicitHeight + 24
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "petra-right-dock"

    BorderSurface {
        anchors.fill: parent
        cornerRadius: Style.radiusPill
        elevated: true

        Column {
            id: dockColumn
            anchors.centerIn: parent
            spacing: 10

            Repeater {
                model: host.appLibrary.apps

                BarIconButton {
                    required property var modelData
                    glyph: modelData.icon
                    tooltip: modelData.name
                    onClicked: host.appLibrary.launch(modelData.exec)
                }
            }

            Rectangle {
                width: 24
                height: 1
                color: Color.borderSubtle
                anchors.horizontalCenter: parent.horizontalCenter
            }

            BarIconButton {
                glyph: "⊞"
                active: Theme.launcherOpen
                onClicked: Theme.toggleLauncher()
            }
        }
    }
}
