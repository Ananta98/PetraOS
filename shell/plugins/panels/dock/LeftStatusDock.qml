import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui

PanelWindow {
    id: root
    required property var host

    anchors {
        left: true
        top: true
        bottom: true
    }

    margins.left: 12
    margins.top: 14
    margins.bottom: 14
    implicitWidth: Style.dockWidth
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "petra-left-dock"

    BorderSurface {
        anchors.fill: parent
        cornerRadius: Style.radiusPill

        Column {
            anchors.top: parent.top
            anchors.topMargin: 12
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 10

            BarIconButton {
                glyph: "◈"
                active: Theme.launcherOpen
                onClicked: Theme.toggleLauncher()
            }

            BarIconButton {
                glyph: "♫"
                active: Theme.mediaPopupOpen && Theme.activeTab === "media"
                onClicked: {
                    Theme.activeTab = "media"
                    Theme.toggleMediaPopup()
                }
            }

            BarIconButton {
                glyph: "◐"
                onClicked: Theme.cycleTheme()
            }
        }

        Column {
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 12
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 10

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "100%"
                font.family: Style.fontMono
                font.pixelSize: 10
                color: Color.success
            }

            BarIconButton {
                glyph: "🔊"
                onClicked: {
                    Theme.activeTab = "media"
                    Theme.toggleMediaPopup()
                }
            }

            BarIconButton {
                glyph: "⏻"
                onClicked: IpcRegistry.launchCommand("poweroff")
            }
        }
    }
}
