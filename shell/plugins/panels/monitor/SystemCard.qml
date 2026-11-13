import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui

PanelWindow {
    id: root
    required property var host

    anchors {
        bottom: true
        left: true
    }

    margins.bottom: 32
    margins.left: 84
    implicitWidth: 360
    implicitHeight: 165
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "petra-system-card"
    exclusionMode: ExclusionMode.Ignore

    BorderSurface {
        anchors.fill: parent
        cornerRadius: Style.radiusCard

        Row {
            anchors.fill: parent
            anchors.margins: 18
            spacing: 18

            // PetraOS Emblem Box
            Rectangle {
                width: 78
                height: 78
                radius: 14
                color: Color.accentSoft
                border.width: 1
                border.color: Color.accent
                anchors.verticalCenter: parent.verticalCenter

                Text {
                    anchors.centerIn: parent
                    text: "◈"
                    font.pixelSize: 38
                    color: Color.accent
                }
            }

            // Fastfetch-style specs
            Column {
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4

                Text {
                    text: "root@petraos"
                    font.family: Style.fontMono
                    font.pixelSize: 13
                    font.bold: true
                    color: Color.accent
                }
                Text {
                    text: "OS: PetraOS (Void Linux glibc)"
                    font.family: Style.fontMono
                    font.pixelSize: 11
                    color: Color.textPrimary
                }
                Text {
                    text: "Kernel: PetraOS 0.1.0 x86_64"
                    font.family: Style.fontMono
                    font.pixelSize: 11
                    color: Color.textSecondary
                }
                Text {
                    text: "WM: Hyprland (Wayland DRM)"
                    font.family: Style.fontMono
                    font.pixelSize: 11
                    color: Color.textSecondary
                }
                Text {
                    text: "Shell: Quickshell (Petra-UI)"
                    font.family: Style.fontMono
                    font.pixelSize: 11
                    color: Color.textSecondary
                }

                // Palette dots
                Row {
                    spacing: 6
                    topPadding: 4
                    Repeater {
                        model: [Color.accent, Color.accentSecondary, Color.success, Color.warning, Color.danger, Color.textPrimary]
                        Rectangle {
                            required property color modelData
                            width: 8
                            height: 8
                            radius: 4
                            color: modelData
                        }
                    }
                }
            }
        }
    }
}
