import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui

PanelWindow {
    id: root
    required property var host

    visible: Theme.mediaPopupOpen

    anchors {
        top: true
    }
    margins.top: Style.topBarHeight + 22
    implicitWidth: 620
    implicitHeight: 220
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "petra-island-popup"

    PopupCard {
        anchors.fill: parent

        // MEDIA TAB (Quickshell Showcase inspired)
        Row {
            visible: Theme.activeTab === "media"
            anchors.centerIn: parent
            spacing: 24

            // Left: Spectrum Ring + Spinning Vinyl Disc
            Item {
                width: 150
                height: 150
                anchors.verticalCenter: parent.verticalCenter

                SpectrumRing {
                    anchors.centerIn: parent
                    active: host.mediaService.playing
                }

                VinylDisc {
                    anchors.centerIn: parent
                    spinning: host.mediaService.playing
                }
            }

            // Center: Track info, controls, timeline
            Column {
                width: 240
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8

                Text {
                    text: host.mediaService.title
                    font.family: Style.fontFamily
                    font.pixelSize: Style.fontSizeTitle
                    font.bold: true
                    color: Color.textPrimary
                    elide: Text.ElideRight
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                }

                Text {
                    text: host.mediaService.artist + " • " + host.mediaService.source
                    font.family: Style.fontFamily
                    font.pixelSize: Style.fontSizeSmall
                    color: Color.textSecondary
                    elide: Text.ElideRight
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                }

                // Controls
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 14

                    BarIconButton {
                        glyph: "⏮"
                        onClicked: host.mediaService.prevTrack()
                    }
                    BarIconButton {
                        glyph: host.mediaService.playing ? "⏸" : "▶"
                        active: true
                        onClicked: host.mediaService.togglePlay()
                    }
                    BarIconButton {
                        glyph: "⏭"
                        onClicked: host.mediaService.nextTrack()
                    }
                }

                // Timeline Slider
                Row {
                    width: parent.width
                    spacing: 8

                    Text {
                        text: Util.formatDuration(host.mediaService.position)
                        font.family: Style.fontMono
                        font.pixelSize: 10
                        color: Color.textMuted
                        anchors.verticalCenter: parent.verticalCenter
                    }

                    PanelSlider {
                        width: parent.width - 72
                        value: host.mediaService.progress
                        anchors.verticalCenter: parent.verticalCenter
                        onMoved: (val) => host.mediaService.seek(val)
                    }

                    Text {
                        text: Util.formatDuration(host.mediaService.duration)
                        font.family: Style.fontMono
                        font.pixelSize: 10
                        color: Color.textMuted
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }

            // Right: Bongo Cat
            BongoCat {
                anchors.verticalCenter: parent.verticalCenter
                active: host.mediaService.playing
            }
        }

        // DASHBOARD TAB
        Column {
            visible: Theme.activeTab === "dashboard"
            anchors.centerIn: parent
            spacing: 12

            Text {
                text: "PetraOS Control Dashboard"
                font.family: Style.fontFamily
                font.pixelSize: 18
                font.bold: true
                color: Color.textPrimary
                anchors.horizontalCenter: parent.horizontalCenter
            }

            Row {
                spacing: 16
                anchors.horizontalCenter: parent.horizontalCenter

                Button {
                    text: "Switch Theme (" + Theme.currentTheme + ")"
                    icon: "◐"
                    active: true
                    onClicked: Theme.cycleTheme()
                }
                Button {
                    text: "Launch Terminal"
                    icon: ">_"
                    onClicked: IpcRegistry.launchCommand("foot")
                }
                Button {
                    text: "App Launcher"
                    icon: "❖"
                    onClicked: {
                        Theme.mediaPopupOpen = false
                        Theme.toggleLauncher()
                    }
                }
            }
        }

        // PERFORMANCE TAB
        Column {
            visible: Theme.activeTab === "performance"
            anchors.centerIn: parent
            spacing: 10

            Text {
                text: "PetraOS Kernel & Subsystem Telemetry"
                font.family: Style.fontFamily
                font.pixelSize: 16
                font.bold: true
                color: Color.textPrimary
                anchors.horizontalCenter: parent.horizontalCenter
            }
            Text {
                text: "Kernel: PetraOS x86_64 (OSTD Rust)  |  Compositor: Hyprland (DRM/KMS)"
                font.family: Style.fontMono
                font.pixelSize: 12
                color: Color.textSecondary
                anchors.horizontalCenter: parent.horizontalCenter
            }
            Text {
                text: "Shell: Quickshell QML (Omarchy Modular Architecture)"
                font.family: Style.fontMono
                font.pixelSize: 12
                color: Color.accent
                anchors.horizontalCenter: parent.horizontalCenter
            }
        }

        // WORKSPACES TAB
        Column {
            visible: Theme.activeTab === "workspaces"
            anchors.centerIn: parent
            spacing: 12

            Text {
                text: "Hyprland Workspaces"
                font.family: Style.fontFamily
                font.pixelSize: 16
                font.bold: true
                color: Color.textPrimary
                anchors.horizontalCenter: parent.horizontalCenter
            }

            Row {
                spacing: 10
                anchors.horizontalCenter: parent.horizontalCenter
                Repeater {
                    model: 5
                    Button {
                        required property int index
                        text: "WS " + (index + 1)
                        onClicked: IpcRegistry.switchWorkspace(index + 1)
                    }
                }
            }
        }
    }
}
