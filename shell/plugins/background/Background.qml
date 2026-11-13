import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

PanelWindow {
    id: root
    required property var host

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }

    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "petra-background"
    exclusionMode: ExclusionMode.Ignore

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.0; color: Color.bgTop }
            GradientStop { position: 0.55; color: Color.bgMid }
            GradientStop { position: 1.0; color: Color.bgBottom }
        }

        // Subtle PetraOS geometric ambient glow
        Rectangle {
            width: parent.width * 0.48
            height: width
            radius: width / 2
            anchors.centerIn: parent
            color: Color.withAlpha(Color.accent, 0.08)
        }

        Rectangle {
            width: parent.width * 0.28
            height: width
            radius: width / 2
            x: parent.width * 0.62
            y: parent.height * 0.14
            color: Color.withAlpha(Color.accentSecondary, 0.07)
        }

        // Subtle watermark signature in center
        Column {
            anchors.centerIn: parent
            spacing: 6
            opacity: 0.16

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "PETRA OS"
                font.family: Style.fontMono
                font.pixelSize: 42
                font.bold: true
                font.letterSpacing: 12
                color: Color.textPrimary
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "HYPRLAND + QUICKSHELL EDITION"
                font.family: Style.fontMono
                font.pixelSize: 12
                font.letterSpacing: 4
                color: Color.accent
            }
        }
    }
}
