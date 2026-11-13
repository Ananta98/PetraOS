import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "widgets"

PanelWindow {
    id: root
    required property var host

    anchors {
        top: true
        left: true
        right: true
    }

    margins.top: 10
    implicitHeight: Style.topBarHeight + 8
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "petra-top-bar"

    BorderSurface {
        id: islandPill
        anchors.centerIn: parent
        width: barContent.implicitWidth + 28
        height: Style.topBarHeight
        cornerRadius: Style.radiusPill
        elevated: true

        Row {
            id: barContent
            anchors.centerIn: parent
            spacing: 14

            // PetraOS Brand Badge
            Row {
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6

                Rectangle {
                    width: 10
                    height: 10
                    radius: 5
                    color: Color.accent
                    anchors.verticalCenter: parent.verticalCenter
                }

                Text {
                    text: "PetraOS"
                    font.family: Style.fontFamily
                    font.pixelSize: Style.fontSizeBody
                    font.bold: true
                    color: Color.textPrimary
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            Rectangle {
                width: 1
                height: 18
                color: Color.borderSubtle
                anchors.verticalCenter: parent.verticalCenter
            }

            // Tab Switcher: Dashboard | Media | Performance | Workspaces
            Tabs {
                host: root.host
                anchors.verticalCenter: parent.verticalCenter
            }

            Rectangle {
                width: 1
                height: 18
                color: Color.borderSubtle
                anchors.verticalCenter: parent.verticalCenter
            }

            // Workspaces
            Workspaces {
                anchors.verticalCenter: parent.verticalCenter
            }

            // Theme Switcher Pill
            Button {
                text: Theme.currentTheme.toUpperCase()
                icon: "◐"
                anchors.verticalCenter: parent.verticalCenter
                onClicked: Theme.cycleTheme()
            }
        }
    }
}
